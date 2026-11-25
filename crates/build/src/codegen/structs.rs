use super::writer::CodeWriter;
use crate::codegen::decode::{get_field_default_expr, lazy_field_helper_name};
use crate::codegen::encode::emit_write_expr;
use crate::core::types::{
    TypeId, emit_serde, is_union_intern_infected, type_info, width_prim_names,
};
use crate::schema::ir::ir_types::*;
use heck::{ToShoutySnakeCase, ToSnakeCase};
use std::collections::{HashMap, HashSet};

fn emit_doc(doc: &[String], w: &mut CodeWriter) {
    for line in doc {
        if line.is_empty() {
            w.line("///");
        } else {
            w.line(&format!("/// {line}"));
        }
    }
}

pub fn emit_structs(schema: &ResolvedSchema, infected: &HashSet<String>, w: &mut CodeWriter) {
    let mut latest: HashMap<String, (i128, &ResolvedType)> = HashMap::new();
    for (type_id, resolved) in &schema.types.types {
        let entry = latest.entry(type_id.name.clone()).or_insert((0, resolved));
        if type_id.version > entry.0 {
            *entry = (type_id.version, resolved);
        }
    }

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        emit_named_struct(
            name,
            &resolved.fields,
            &resolved.const_fields,
            &resolved.doc,
            infected,
            schema,
            w,
        );
        w.blank();
    }

    let latest_version = schema.versions.last().expect("no versions");
    emit_named_struct(
        &schema.name_hint,
        &latest_version.fields,
        &latest_version.const_fields,
        &schema.doc,
        infected,
        schema,
        w,
    );
    w.blank();
}

pub fn emit_enums(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.enums.enums);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        emit_enum(name, resolved, w);
        w.blank();
    }
}

fn emit_enum(name: &str, resolved: &ResolvedEnum, w: &mut CodeWriter) {
    let has_default = !resolved.variants.is_empty();
    let serde_derive = if emit_serde() {
        ", serde::Serialize, serde::Deserialize"
    } else {
        ""
    };
    emit_doc(&resolved.doc, w);
    if has_default {
        w.line(&format!(
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, Default{serde_derive})]"
        ));
    } else {
        w.line(&format!(
            "#[derive(Debug, Clone, Copy, PartialEq, Eq{serde_derive})]"
        ));
    }
    w.line("#[repr(u32)]");
    w.line(&format!("pub enum {name} {{"));
    w.indent();
    for (i, variant) in resolved.variants.iter().enumerate() {
        emit_doc(&variant.doc, w);
        if i == 0 && has_default {
            w.line("#[default]");
        }
        w.line(&format!("{} = {},", variant.name, variant.wire_value));
    }
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl TryFrom<u32> for {name} {{"));
    w.indent();
    w.line("type Error = u32;");
    w.line("fn try_from(v: u32) -> std::result::Result<Self, u32> {");
    w.indent();
    w.line("match v {");
    w.indent();
    for variant in &resolved.variants {
        w.line(&format!(
            "{} => Ok({name}::{}),",
            variant.wire_value, variant.name
        ));
    }
    w.line("other => Err(other),");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
}

pub fn emit_unions(schema: &ResolvedSchema, infected: &HashSet<String>, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.unions.unions);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (version, resolved) = latest[name];
        emit_union(name, version, resolved, infected, w);
        w.blank();
    }
}

fn emit_union(
    name: &str,
    latest_version: i128,
    resolved: &ResolvedUnion,
    infected: &HashSet<String>,
    w: &mut CodeWriter,
) {
    let needs_lifetime = infected.contains(name);
    let enum_lt = if needs_lifetime { "<'buf>" } else { "" };

    // lifetime-infected unions get a hand-written impl below, see emit_lazy_struct_serde.
    let serde_derive = if emit_serde() && !needs_lifetime {
        ", serde::Serialize, serde::Deserialize"
    } else {
        ""
    };
    // Unknown stores its discriminant at the union's latest width, which is
    // always wide enough since width only ever grows.
    let (unknown_rust_int, _, _) = width_prim_names(resolved.discriminant_width());

    emit_doc(&resolved.doc, w);
    w.line(&format!("#[derive(Debug, Clone{serde_derive})]"));
    w.line(&format!("pub enum {name}{enum_lt} {{"));
    w.indent();
    for variant in &resolved.variants {
        emit_doc(&variant.doc, w);
        w.line(&format!(
            "{}({}),",
            variant.name,
            type_info(&variant.payload).rust_type
        ));
    }
    w.line(&format!(
        "Unknown {{ discriminant: {unknown_rust_int}, data: Vec<u8> }},"
    ));
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl{enum_lt} Default for {name}{enum_lt} {{"));
    w.indent();
    w.line("fn default() -> Self {");
    w.indent();
    if let Some(first) = resolved.variants.first() {
        w.line(&format!("{name}::{}(Default::default())", first.name));
    } else {
        w.line(&format!(
            "{name}::Unknown {{ discriminant: 0, data: Vec::new() }}"
        ));
    }
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl{enum_lt} {name}{enum_lt} {{"));
    w.indent();
    w.line("#[allow(dead_code)]");
    w.line("pub const fn discriminant(&self) -> u64 {");
    w.indent();
    w.line("match self {");
    w.indent();
    for variant in &resolved.variants {
        w.line(&format!(
            "{name}::{}(_) => {},",
            variant.name, variant.discriminant
        ));
    }
    w.line(&format!(
        "{name}::Unknown {{ discriminant, .. }} => *discriminant as u64,"
    ));
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");

    if needs_lifetime && emit_serde() {
        emit_lazy_union_serde(name, latest_version, resolved, w);
    }
}

fn emit_lazy_union_serde(
    name: &str,
    latest_version: i128,
    resolved: &ResolvedUnion,
    w: &mut CodeWriter,
) {
    let lower = name.to_snake_case();
    let interning = is_union_intern_infected(&TypeId {
        name: name.to_string(),
        version: latest_version,
    });

    w.blank();
    w.line(&format!("impl<'buf> serde::Serialize for {name}<'buf> {{"));
    w.indent();
    w.line("fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {");
    w.indent();
    w.line("if serializer.is_human_readable() {");
    w.indent();
    w.line("return match self {");
    w.indent();
    for (index, variant) in resolved.variants.iter().enumerate() {
        let variant = &variant.name;
        w.line(&format!(
            "Self::{variant}(value) => serializer.serialize_newtype_variant(\"{name}\", {index}, \"{variant}\", value),"
        ));
    }
    let unknown_index = resolved.variants.len();
    w.line("Self::Unknown { discriminant, data } => {");
    w.indent();
    w.line("use serde::ser::SerializeStructVariant;");
    w.line(&format!(
        "let mut variant = serializer.serialize_struct_variant(\"{name}\", {unknown_index}, \"Unknown\", 2)?;"
    ));
    w.line("variant.serialize_field(\"discriminant\", discriminant)?;");
    w.line("variant.serialize_field(\"data\", data)?;");
    w.line("variant.end()");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("};");
    w.dedent();
    w.line("}");
    // Binary Serde formats continue to serialize an opaque wire payload.
    if interning {
        // same table-must-precede-fields dance as encode_vN: encode into a
        // scratch buffer first so every intern() call happens before the
        // table is finished, then prepend the table.
        w.line("let mut __interner_owned = InternBuilder::new();");
        w.line("let mut __scratch: Vec<u8> = Vec::new();");
        w.line(&format!(
            "write_{lower}_v{latest_version}(&mut __scratch, self, &mut __interner_owned).map_err(serde::ser::Error::custom)?;"
        ));
        w.line("let mut __buf = Vec::new();");
        w.line("write_intern_table(&mut __buf, &__interner_owned.finish_table());");
        w.line("__buf.extend_from_slice(&__scratch);");
    } else {
        w.line("let mut __buf = Vec::new();");
        // union writers are always fallible now, so this always needs map_err.
        w.line(&format!(
            "write_{lower}_v{latest_version}(&mut __buf, self).map_err(serde::ser::Error::custom)?;"
        ));
    }
    w.line("serializer.serialize_bytes(&__buf)");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!(
        "impl<'de: 'buf, 'buf> serde::Deserialize<'de> for {name}<'buf> {{"
    ));
    w.indent();
    w.line(
        "fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {",
    );
    w.indent();
    w.line("if deserializer.is_human_readable() {");
    w.indent();
    w.line("#[derive(serde::Deserialize)]");
    w.line(&format!("#[serde(rename = \"{name}\")]"));
    w.line("enum HumanReadable<'buf> {");
    w.indent();
    for variant in &resolved.variants {
        let ty = type_info(&variant.payload).rust_type;
        let borrow = if ty.contains("'buf") {
            "#[serde(borrow)] "
        } else {
            ""
        };
        w.line(&format!("{}({borrow}{ty}),", variant.name));
    }
    let (discriminant_type, _, _) = width_prim_names(resolved.discriminant_width());
    w.line(&format!(
        "Unknown {{ discriminant: {discriminant_type}, data: Vec<u8> }},"
    ));
    w.dedent();
    w.line("}");
    w.line("return Ok(match serde::Deserialize::deserialize(deserializer)? {");
    w.indent();
    for variant in &resolved.variants {
        let variant = &variant.name;
        w.line(&format!(
            "HumanReadable::{variant}(value) => Self::{variant}(value),"
        ));
    }
    w.line(
        "HumanReadable::Unknown { discriminant, data } => Self::Unknown { discriminant, data },",
    );
    w.dedent();
    w.line("});");
    w.dedent();
    w.line("}");
    w.line("struct __Visitor<'buf>(std::marker::PhantomData<&'buf ()>);");
    w.line("impl<'de: 'buf, 'buf> serde::de::Visitor<'de> for __Visitor<'buf> {");
    w.indent();
    w.line(&format!("type Value = {name}<'buf>;"));
    w.line("fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {");
    w.indent();
    w.line(&format!("write!(f, \"bytes for {name}\")"));
    w.dedent();
    w.line("}");
    w.blank();
    w.line(
        "fn visit_borrowed_bytes<E: serde::de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {",
    );
    w.indent();
    w.line("let mut __pos = 0;");
    if interning {
        w.line("let __table_owned = read_intern_table(v, &mut __pos).map_err(serde::de::Error::custom)?;");
        w.line("let __table = __table_owned.as_slice();");
        w.line(&format!(
            "read_{lower}_v{latest_version}(v, &mut __pos, __table).map_err(serde::de::Error::custom)"
        ));
    } else {
        w.line(&format!(
            "read_{lower}_v{latest_version}(v, &mut __pos).map_err(serde::de::Error::custom)"
        ));
    }
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();
    w.line("deserializer.deserialize_bytes(__Visitor(std::marker::PhantomData))");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
}

pub fn emit_bitsets(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.bitsets.bitsets);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, bs) = latest[name];
        emit_bitset_struct(name, bs, schema, w);
        w.blank();
    }
}

fn emit_bitset_struct(
    name: &str,
    bs: &ResolvedBitset,
    schema: &ResolvedSchema,
    w: &mut CodeWriter,
) {
    let computed_len = bs.byte_width();

    let serde_derive = if emit_serde() {
        ", serde::Serialize, serde::Deserialize"
    } else {
        ""
    };
    emit_doc(&bs.doc, w);
    // needed for sorting and use as hashmap/hashset keys
    w.line(&format!(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash{serde_derive})]"
    ));
    w.line(&format!("pub struct {name}(pub [u8; {computed_len}]);"));
    w.blank();

    w.line(&format!("impl {name} {{"));
    w.indent();

    for (idx, variant) in bs.variants.iter().enumerate() {
        if variant.name.starts_with("__deprecated_") {
            continue;
        }
        let upper = variant.name.to_shouty_snake_case();
        let byte_idx = idx / 8;
        let bit_idx = idx % 8;
        let mut bytes = vec![0u8; computed_len];
        bytes[byte_idx] |= 1 << bit_idx;
        let inner = bytes
            .iter()
            .map(|b| format!("0x{b:02x}"))
            .collect::<Vec<_>>()
            .join(", ");
        emit_doc(&variant.doc, w);
        w.line(&format!("pub const {upper}: Self = Self([{inner}]);"));
    }
    w.blank();

    w.line("#[inline]");
    w.line("pub const fn is_empty(&self) -> bool {");
    w.indent();
    let empty_checks = (0..computed_len)
        .map(|i| format!("self.0[{i}] == 0"))
        .collect::<Vec<_>>()
        .join(" && ");
    w.line(empty_checks.as_str());
    w.dedent();
    w.line("}");
    w.blank();

    for (idx, variant) in bs.variants.iter().enumerate() {
        if variant.name.starts_with("__deprecated_") {
            continue;
        }
        let lower = variant.name.to_snake_case();
        let byte_idx = idx / 8;
        let bit_idx = idx % 8;

        w.line("#[inline]");
        w.line(&format!(
            "pub const fn {lower}(&self) -> bool {{ (self.0[{byte_idx}] & (1 << {bit_idx})) != 0 }}"
        ));

        w.line("#[inline]");
        w.line(&format!("pub fn set_{lower}(&mut self, val: bool) {{"));
        w.indent();
        w.line(&format!("if val {{ self.0[{byte_idx}] |= 1 << {bit_idx}; }} else {{ self.0[{byte_idx}] &= !(1 << {bit_idx}); }}"));
        w.dedent();
        w.line("}");

        w.line("#[inline]");
        w.line(&format!(
            "pub const fn with_{lower}(mut self, val: bool) -> Self {{ \
                if val {{ self.0[{byte_idx}] |= 1 << {bit_idx}; }} \
                else {{ self.0[{byte_idx}] &= !(1 << {bit_idx}); }} \
                self \
            }}"
        ));
    }

    w.dedent();
    w.line("}");
    w.blank();

    let mut default_bytes = vec![0u8; computed_len];
    if let Some(DefaultValue::BitsetLiteral { kvs, .. }) = find_bitset_default(name, schema) {
        for (flag_name, flag_val) in kvs {
            if *flag_val && let Some(idx) = bs.variants.iter().position(|v| v.name == *flag_name) {
                default_bytes[idx / 8] |= 1 << (idx % 8);
            }
        }
    }
    let inner = default_bytes
        .iter()
        .map(|b| format!("0x{b:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    w.line(&format!("impl Default for {name} {{"));
    w.indent();
    w.line("#[inline]");
    w.line("fn default() -> Self {");
    w.indent();
    w.line(&format!("{name}([{inner}])"));
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl ::std::ops::BitOr for {name} {{"));
    w.indent();
    w.line("type Output = Self;");
    w.line("#[inline]");
    w.line("fn bitor(mut self, rhs: Self) -> Self {");
    w.indent();
    for i in 0..computed_len {
        w.line(&format!("self.0[{i}] |= rhs.0[{i}];"));
    }
    w.line("self");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl ::std::ops::BitOrAssign for {name} {{"));
    w.indent();
    w.line("#[inline]");
    w.line("fn bitor_assign(&mut self, rhs: Self) {");
    w.indent();
    for i in 0..computed_len {
        w.line(&format!("self.0[{i}] |= rhs.0[{i}];"));
    }
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl ::std::ops::BitAnd for {name} {{"));
    w.indent();
    w.line("type Output = Self;");
    w.line("#[inline]");
    w.line("fn bitand(mut self, rhs: Self) -> Self {");
    w.indent();
    for i in 0..computed_len {
        w.line(&format!("self.0[{i}] &= rhs.0[{i}];"));
    }
    w.line("self");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl ::std::ops::BitAndAssign for {name} {{"));
    w.indent();
    w.line("#[inline]");
    w.line("fn bitand_assign(&mut self, rhs: Self) {");
    w.indent();
    for i in 0..computed_len {
        w.line(&format!("self.0[{i}] &= rhs.0[{i}];"));
    }
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl ::std::ops::Not for {name} {{"));
    w.indent();
    w.line("type Output = Self;");
    w.line("#[inline]");
    w.line("fn not(mut self) -> Self {");
    w.indent();
    for i in 0..computed_len {
        w.line(&format!("self.0[{i}] = !self.0[{i}];"));
    }
    w.line("self");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
}

fn find_bitset_default<'a>(name: &str, schema: &'a ResolvedSchema) -> Option<&'a DefaultValue> {
    for version in &schema.versions {
        for field in &version.fields {
            if let Some(DefaultValue::BitsetLiteral { ty_name, .. }) = &field.default
                && ty_name == name
            {
                return field.default.as_ref();
            }
        }
    }
    None
}

fn emit_named_struct(
    name: &str,
    fields: &[FieldIR],
    consts: &[ResolvedConst],
    doc: &[String],
    infected: &HashSet<String>,
    schema: &ResolvedSchema,
    w: &mut CodeWriter,
) {
    let needs_lifetime = infected.contains(name);
    let struct_lt = if needs_lifetime { "<'buf>" } else { "" };
    let impl_lt_param = if needs_lifetime { "<'buf>" } else { "" };

    let serde_derive = if emit_serde() {
        ", serde::Serialize, serde::Deserialize"
    } else {
        ""
    };
    emit_doc(doc, w);
    w.line("#[allow(clippy::type_complexity, reason = \"field types follow the schema\")]");
    if needs_lifetime {
        // a lifetime-infected struct may hold a `lazy` field (LazyView), which
        // needs a hand-written Serialize/Deserialize impl below rather than a
        // derive — see emit_lazy_struct_serde
        w.line("#[derive(Debug, Clone)]");
    } else {
        w.line(&format!("#[derive(Debug, Clone{serde_derive})]"));
    }

    w.line(&format!("pub struct {name}{struct_lt} {{"));
    w.indent();
    for field in fields {
        // type_info already renders <'buf> on infected types, only lazy needs wrapping
        let ty = if field.lazy {
            let inner = type_info(&field.ty).rust_type;
            format!("LazyView<'buf, {inner}>")
        } else {
            type_info(&field.ty).rust_type
        };
        emit_doc(&field.doc, w);
        w.line(&format!("pub {}: {ty},", field.name));
    }
    w.dedent();
    w.line("}");

    // manual (not derived) so a field's declared `= value` schema default —
    // not just its Rust type's zero value — backs `T::default()`
    w.blank();
    w.line(&format!(
        "impl{impl_lt_param} Default for {name}{struct_lt} {{"
    ));
    w.indent();
    w.line("#[inline]");
    w.line("fn default() -> Self {");
    w.indent();
    w.line("Self {");
    w.indent();
    for field in fields {
        if field.lazy {
            let default_expr = type_info(&field.ty).default_expr;
            w.line(&format!("{}: LazyView::Owned({default_expr}),", field.name));
        } else {
            let default_expr = get_field_default_expr(field.default.as_ref(), &field.ty, schema);
            w.line(&format!("{}: {default_expr},", field.name));
        }
    }
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");

    if !consts.is_empty() {
        w.blank();
        w.line(&format!("impl{impl_lt_param} {name}{struct_lt} {{"));
        w.indent();
        for c in consts {
            let const_name = c.name.to_shouty_snake_case();
            let value = render_const_value(&c.value);
            emit_doc(&c.doc, w);
            if matches!(c.value, DefaultValue::Float(_)) {
                w.line(
                    "#[allow(clippy::approx_constant, reason = \"preserve the schema literal\")]",
                );
            }
            w.line(&format!(
                "pub const {const_name}: {} = {value};",
                c.rust_type
            ));
        }
        w.dedent();
        w.line("}");
    }

    if needs_lifetime && emit_serde() {
        emit_lazy_struct_serde(name, fields, schema, w);
    }
}

fn render_const_value(value: &DefaultValue) -> String {
    match value {
        DefaultValue::Bool(b) => b.to_string(),
        DefaultValue::Int(i) => i.to_string(),
        DefaultValue::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        DefaultValue::Str(s) => format!("\"{s}\""),
        _ => unreachable!("const fields only hold primitive values"),
    }
}

fn emit_lazy_struct_serde(
    name: &str,
    fields: &[FieldIR],
    schema: &ResolvedSchema,
    w: &mut CodeWriter,
) {
    w.blank();

    w.line(&format!("impl<'buf> serde::Serialize for {name}<'buf> {{"));
    w.indent();
    w.line("fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {");
    w.indent();
    w.line("use serde::ser::SerializeStruct;");
    if fields.iter().any(|field| field.lazy) {
        w.line("let __human_readable = serializer.is_human_readable();");
    }
    w.line(&format!(
        "let mut __s = serializer.serialize_struct(\"{name}\", {})?;",
        fields.len()
    ));
    for f in fields {
        if f.lazy {
            w.line("if __human_readable {");
            w.indent();
            w.line(&format!(
                "__s.serialize_field(\"{n}\", &self.{n})?;",
                n = f.name
            ));
            w.dedent();
            w.line("} else {");
            w.indent();
            w.line(&format!("match &self.{n} {{", n = f.name));
            w.indent();
            w.line(&format!(
                "LazyView::Raw {{ buf, .. }} => __s.serialize_field(\"{n}\", SerdeBytes::new(buf))?,",
                n = f.name
            ));
            w.line("LazyView::Owned(__val) => {");
            w.indent();
            // Keep codec errors local until the serialization boundary.
            w.line("let __write = |__buf: &mut Vec<u8>| -> PojocResult<()> {");
            w.indent();
            emit_write_expr(&f.ty, "__val", Some(schema), true, w);
            w.line("Ok(())");
            w.dedent();
            w.line("};");
            w.line("let mut __buf = Vec::new();");
            w.line("__write(&mut __buf).map_err(serde::ser::Error::custom)?;");
            w.line(&format!(
                "__s.serialize_field(\"{}\", SerdeBytes::new(&__buf))?;",
                f.name
            ));
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
        } else {
            w.line(&format!(
                "__s.serialize_field(\"{n}\", &self.{n})?;",
                n = f.name
            ));
        }
    }
    w.line("__s.end()");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    let field_list = fields
        .iter()
        .map(|f| format!("\"{}\"", f.name))
        .collect::<Vec<_>>()
        .join(", ");

    w.line(&format!(
        "impl<'de: 'buf, 'buf> serde::Deserialize<'de> for {name}<'buf> {{"
    ));
    w.indent();
    w.line(
        "fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {",
    );
    w.indent();
    w.line("if deserializer.is_human_readable() {");
    w.indent();
    w.line("#[derive(serde::Deserialize)]");
    w.line("#[allow(clippy::type_complexity, reason = \"field types follow the schema\")]");
    w.line(&format!("#[serde(rename = \"{name}\")]"));
    w.line("struct HumanReadable<'buf> {");
    w.indent();
    for field in fields {
        let mut ty = type_info(&field.ty).rust_type;
        if field.lazy {
            ty = format!("LazyView<'buf, {ty}>");
        }
        if ty.contains("'buf") {
            w.line("#[serde(borrow)]");
        }
        w.line(&format!("{}: {ty},", field.name));
    }
    w.dedent();
    w.line("}");
    w.line("let value: HumanReadable<'_> = serde::Deserialize::deserialize(deserializer)?;");
    w.line("return Ok(Self {");
    w.indent();
    for field in fields {
        let name = &field.name;
        w.line(&format!("{name}: value.{name},"));
    }
    w.dedent();
    w.line("});");
    w.dedent();
    w.line("}");
    w.line("struct __Visitor<'buf>(std::marker::PhantomData<&'buf ()>);");
    w.line("impl<'de: 'buf, 'buf> serde::de::Visitor<'de> for __Visitor<'buf> {");
    w.indent();
    w.line(&format!("type Value = {name}<'buf>;"));
    w.line("fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {");
    w.indent();
    w.line(&format!("write!(f, \"struct {name}\")"));
    w.dedent();
    w.line("}");
    w.blank();
    w.line("fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {");
    w.indent();
    for (idx, f) in fields.iter().enumerate() {
        if f.lazy {
            w.line(&format!(
                "let __{n}_b: &'de SerdeBytes = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length({idx}, &\"{name}\"))?;",
                n = f.name
            ));
            w.line(&format!("let __{n}: &'de [u8] = &__{n}_b[..];", n = f.name));
        } else {
            w.line(&format!(
                "let {n} = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length({idx}, &\"{name}\"))?;",
                n = f.name
            ));
        }
    }
    w.line(&format!("Ok({name} {{"));
    w.indent();
    for f in fields {
        if f.lazy {
            let helper_name = lazy_field_helper_name(name, &f.name);
            w.line(&format!(
                "{n}: LazyView::new(__{n}, {helper_name}),",
                n = f.name
            ));
        } else {
            w.line(&format!("{n},", n = f.name));
        }
    }
    w.dedent();
    w.line("})");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();
    w.line(&format!(
        "deserializer.deserialize_struct(\"{name}\", &[{field_list}], __Visitor(std::marker::PhantomData))"
    ));
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
}
