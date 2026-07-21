use super::writer::CodeWriter;
use crate::codegen::encode::emit_write_expr;
use crate::codegen::get_latest_versions;
use crate::core::types::type_info;
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
    let latest = get_latest_versions(&schema.enums.enums, |id| id.name.clone(), |id| id.version);

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
    let serde_derive = if cfg!(feature = "serde") {
        ", Serialize, Deserialize"
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

pub fn emit_unions(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.unions.unions, |id| id.name.clone(), |id| id.version);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        emit_union(name, resolved, w);
        w.blank();
    }
}

fn emit_union(name: &str, resolved: &ResolvedUnion, w: &mut CodeWriter) {
    let serde_derive = if cfg!(feature = "serde") {
        ", Serialize, Deserialize"
    } else {
        ""
    };
    emit_doc(&resolved.doc, w);
    w.line(&format!("#[derive(Debug, Clone{serde_derive})]"));
    w.line(&format!("pub enum {name} {{"));
    w.indent();
    for variant in &resolved.variants {
        emit_doc(&variant.doc, w);
        w.line(&format!(
            "{}({}),",
            variant.name,
            type_info(&variant.payload).rust_type
        ));
    }
    w.line("Unknown { discriminant: u64, data: Vec<u8> },");
    w.dedent();
    w.line("}");
    w.blank();

    w.line(&format!("impl Default for {name} {{"));
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

    w.line(&format!("impl {name} {{"));
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
        "{name}::Unknown {{ discriminant, .. }} => *discriminant,"
    ));
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
}

pub fn emit_bitsets(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(
        &schema.bitsets.bitsets,
        |id| id.name.clone(),
        |id| id.version,
    );

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, bs) = latest[name];
        emit_bitset_struct(name, bs, schema, w); // Pass schema to handle default calculations
        w.blank();
    }
}

fn emit_bitset_struct(
    name: &str,
    bs: &ResolvedBitset,
    schema: &ResolvedSchema,
    w: &mut CodeWriter,
) {
    let computed_len = bs.variants.len().div_ceil(8);

    let serde_derive = if cfg!(feature = "serde") {
        ", Serialize, Deserialize"
    } else {
        ""
    };
    emit_doc(&bs.doc, w);
    // Added PartialOrd, Ord, and Hash so these can be sorted or used as keys in a HashMap/HashSet
    w.line(&format!(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash{serde_derive})]"
    ));
    w.line(&format!("pub struct {name}(pub [u8; {computed_len}]);"));
    w.blank();

    w.line(&format!("impl {name} {{"));
    w.indent();

    // const flags
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

    // Added a helper to check if any flags are active
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

    // getters, setters, builders (all decorated with #[inline])
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

    // Default
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

    // BitOr
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

    // BitOrAssign
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

    // BitAnd
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

    // BitAndAssign
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

    // Not
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

    let serde_derive = if cfg!(feature = "serde") {
        ", Serialize, Deserialize"
    } else {
        ""
    };
    emit_doc(doc, w);
    w.line("#[allow(clippy::type_complexity)]");
    if needs_lifetime {
        w.line("#[derive(Debug, Clone)]");
    } else {
        w.line(&format!("#[derive(Debug, Clone, Default{serde_derive})]"));
    }

    w.line(&format!("pub struct {name}{struct_lt} {{"));
    w.indent();
    for field in fields {
        // `type_info` already renders infected named types (and borrowed
        // strings) with `<'buf>` wherever they nest, so a plain lookup is
        // correct for every non-lazy field; only lazy needs the wrapper.
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

    if needs_lifetime {
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
                w.line(&format!("{}: Default::default(),", field.name));
            }
        }
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    }

    if !consts.is_empty() {
        w.blank();
        w.line("#[allow(clippy::approx_constant)]");
        w.line(&format!("impl{impl_lt_param} {name}{struct_lt} {{"));
        w.indent();
        for c in consts {
            let const_name = c.name.to_shouty_snake_case();
            let value = render_const_value(&c.value);
            emit_doc(&c.doc, w);
            w.line(&format!(
                "pub const {const_name}: {} = {value};",
                c.rust_type
            ));
        }
        w.dedent();
        w.line("}");
    }

    if needs_lifetime && cfg!(feature = "serde") {
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

    // Serialize
    w.line(&format!("impl<'buf> Serialize for {name}<'buf> {{"));
    w.indent();
    w.line("fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {");
    w.indent();
    w.line("use serde::ser::SerializeStruct;");
    w.line(&format!(
        "let mut __s = serializer.serialize_struct(\"{name}\", {})?;",
        fields.len()
    ));
    for f in fields {
        if f.lazy {
            w.line(&format!("match &self.{n} {{", n = f.name));
            w.indent();
            w.line(&format!(
                "LazyView::Raw {{ buf, .. }} => __s.serialize_field(\"{n}\", SerdeBytes::new(buf))?,",
                n = f.name
            ));
            w.line("LazyView::Owned(__val) => {");
            w.indent();
            w.line("let mut __buf = Vec::new();");
            w.line("let __buf = &mut __buf;");
            emit_write_expr(&f.ty, "__val", Some(schema), true, w);
            w.line(&format!(
                "__s.serialize_field(\"{}\", SerdeBytes::new(__buf))?;",
                f.name
            ));
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

    // Deserialize
    let field_list = fields
        .iter()
        .map(|f| format!("\"{}\"", f.name))
        .collect::<Vec<_>>()
        .join(", ");

    w.line(&format!(
        "impl<'de: 'buf, 'buf> Deserialize<'de> for {name}<'buf> {{"
    ));
    w.indent();
    w.line(
        "fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {",
    );
    w.indent();
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
            w.line(&format!("{n}: LazyView::new(__{n}, {n}_read),", n = f.name));
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
