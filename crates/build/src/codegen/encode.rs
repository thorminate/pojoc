use super::writer::CodeWriter;
use crate::codegen::{
    emit_constraint_guard, emit_union_variant_constraint_guard, ordered_wire_fields,
};
use crate::core::types::*;
use crate::schema::ir::ir_types::*;
use crate::schema::ir::lineage::*;
use heck::ToSnakeCase;

pub fn emit_encode_helpers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    emit_bitset_writers(schema, w);
    emit_type_writers(schema, w);
    emit_union_writers(schema, w);
}

pub fn emit_encode_vn_functions(
    schema: &ResolvedSchema,
    infected: &std::collections::HashSet<String>,
    w: &mut CodeWriter,
) {
    let lt = if infected.contains(schema.name_hint.as_str()) {
        "<'buf>"
    } else {
        ""
    };
    for vl in &schema.lineage.versions {
        emit_encode_vn_fn(schema, vl, lt, w);
        w.blank();
    }
}

pub fn emit_size_hint_helpers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    emit_type_size_hints(schema, w);
    emit_union_size_hints(schema, w);
}

pub fn emit_size_hint_vn_functions(
    schema: &ResolvedSchema,
    infected: &std::collections::HashSet<String>,
    w: &mut CodeWriter,
) {
    let lt = if infected.contains(schema.name_hint.as_str()) {
        "<'buf>"
    } else {
        ""
    };
    for vl in &schema.lineage.versions {
        emit_size_hint_vn_fn(schema, vl, lt, w);
        w.blank();
    }
}

fn deref_if_copy(ty: &ResolvedTypeRef, var: &str) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id)
            if is_primitive(&id.name) && normalize_type(&id.name) != "string" =>
        {
            format!("*{var}")
        }
        ResolvedTypeRef::VFloat { .. } => format!("*{var}"),
        // transparent on the wire — look through to the underlying scalar
        ResolvedTypeRef::Constrained { inner, .. } => deref_if_copy(inner, var),
        _ => var.to_string(),
    }
}

// one writer per resolved union version, frozen at that version's own
// discriminant width forever — see ResolvedUnion::discriminant_width.
fn emit_union_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest_by_name = latest_by_name(&schema.unions.unions);

    let mut entries: Vec<(&TypeId, &ResolvedUnion)> = schema.unions.unions.iter().collect();
    entries
        .sort_by(|(a, _), (b, _)| (a.name.as_str(), a.version).cmp(&(b.name.as_str(), b.version)));

    for (id, resolved) in entries {
        let name = &id.name;
        let (_, latest) = latest_by_name[name];
        let width = resolved.discriminant_width();
        let max_value = width_max_value(width);
        let (rust_int, _, write_prim) = width_prim_names(width);
        // only emit the overflow guard when this version's width is actually
        // narrower than Unknown's (latest) width — otherwise it's dead code.
        let overflow_possible = width < latest.discriminant_width();
        let lt = if is_infected(name) { "<'buf>" } else { "" };
        let interning = is_union_write_intern_infected(id);
        let extra_param = if interning {
            ", __interner: &mut InternBuilder<'buf>"
        } else {
            ""
        };
        let fn_name = format!("write_{}_v{}", name.to_snake_case(), id.version);
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}{lt}(__buf: &mut Vec<u8>, __value: &{name}{lt}{extra_param}) -> PojocResult<()> {{"
        ));
        w.indent();
        w.line("match __value {");
        w.indent();
        for variant in &latest.variants {
            if variant.discriminant > max_value {
                w.line(&format!(
                    "{name}::{}(_) => return Err(Error::DiscriminantOverflow),",
                    variant.name
                ));
                continue;
            }
            w.line(&format!("{name}::{}(__payload) => {{", variant.name));
            w.indent();
            emit_union_variant_constraint_guard(&variant.payload, "__payload", &variant.name, w);
            // suffixed literal (`3u8`), not `3 as u8` — clippy flags a redundant cast on a constant.
            w.line(&format!(
                "{write_prim}(__buf, {}{rust_int});",
                variant.discriminant
            ));
            w.line("let mut __tmp = Vec::new();");
            emit_union_payload_write(&variant.payload, w);
            w.line("write_varint32(__buf, __tmp.len() as u32);");
            w.line("__buf.extend_from_slice(&__tmp);");
            w.dedent();
            w.line("}");
        }
        w.line(&format!("{name}::Unknown {{ discriminant, data }} => {{"));
        w.indent();
        if overflow_possible {
            w.line(&format!(
                "if *discriminant > {max_value} {{ return Err(Error::DiscriminantOverflow); }}"
            ));
            w.line(&format!(
                "{write_prim}(__buf, *discriminant as {rust_int});"
            ));
        } else {
            // no cast needed: declared type already matches this version's width.
            w.line(&format!("{write_prim}(__buf, *discriminant);"));
        }
        w.line("write_varint32(__buf, data.len() as u32);");
        w.line("__buf.extend_from_slice(data);");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
        w.line("Ok(())");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_bitset_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.bitsets.bitsets);
    let mut versions: Vec<_> = schema.bitsets.bitsets.iter().collect();
    versions.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
    for (id, bitset) in versions {
        let name = &id.name;
        let width = bitset.byte_width();
        let latest_width = latest[name].1.byte_width();
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn write_{}_v{}(__buf: &mut Vec<u8>, __value: &{name}) {{",
            name.to_snake_case(),
            id.version
        ));
        w.indent();
        if width == latest_width {
            w.line("write_fixed_bytes(__buf, &__value.0);");
        } else if width < latest_width {
            w.line(&format!("__buf.extend_from_slice(&__value.0[..{width}]);"));
        } else {
            w.line(&format!("let mut flags = [0u8; {width}];"));
            w.line(&format!(
                "flags[..{latest_width}].copy_from_slice(&__value.0);"
            ));
            w.line("write_fixed_bytes(__buf, &flags);");
        }
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_type_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.types.types);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();
    for name in names {
        let (_, resolved) = latest[name];
        let (__buf, val) = if resolved.fields.is_empty() {
            ("_buf", "_value")
        } else {
            ("__buf", "__value")
        };
        let lt = if is_infected(name) { "<'buf>" } else { "" };
        let fallible = is_constraint_infected(name);
        let ret = if fallible { " -> PojocResult<()>" } else { "" };
        let interning = is_intern_infected(name);
        let extra_param = if interning {
            ", __interner: &mut InternBuilder<'buf>"
        } else {
            ""
        };
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn write_{}{lt}({__buf}: &mut Vec<u8>, {val}: &{name}{lt}{extra_param}){ret} {{",
            name.to_snake_case()
        ));
        w.indent();
        if fallible {
            for field in &resolved.fields {
                emit_constraint_guard(field, &format!("{val}.{}", field.name), w);
            }
        }
        emit_optional_header_write(&resolved.fields, w);
        emit_fields_write_loop(schema, &resolved.fields, w);
        if fallible {
            w.line("Ok(())");
        }
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_union_payload_write(payload: &ResolvedTypeRef, w: &mut CodeWriter) {
    match payload {
        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => {
            // __payload is already &T here
            let q = if is_constraint_infected(&id.name) {
                "?"
            } else {
                ""
            };
            let extra = if is_intern_infected(&id.name) {
                ", __interner"
            } else {
                ""
            };
            w.line(&format!(
                "write_{}(&mut __tmp, __payload{extra}){q};",
                id.name.to_snake_case()
            ));
        }
        _ => {
            w.line("{");
            w.indent();
            w.line("let __buf = &mut __tmp;");
            let accessor = deref_if_copy(payload, "__payload");
            emit_write_expr(payload, &accessor, None, true, w);
            w.dedent();
            w.line("}");
        }
    }
}

fn emit_optional_header_write(fields: &[FieldIR], w: &mut CodeWriter) {
    let opt: Vec<&FieldIR> = fields
        .iter()
        .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
        .collect();
    let header_bytes = opt.len().div_ceil(8);
    if header_bytes == 0 {
        return;
    }
    w.line(&format!("let mut __header = [0u8; {header_bytes}];"));
    for (idx, f) in opt.iter().enumerate() {
        w.line(&format!(
            "if __value.{}.is_some() {{ __header[{}] |= 1 << {}; }}",
            f.name,
            idx / 8,
            idx % 8
        ));
    }
    w.line("write_fixed_bytes(__buf, &__header);");
}

fn emit_fields_write_loop(schema: &ResolvedSchema, fields: &[FieldIR], w: &mut CodeWriter) {
    for field in fields {
        if field.lazy {
            emit_lazy_encode(&field.name, &field.ty, schema, w);
            continue;
        }
        let accessor = format!("__value.{}", field.name);
        match &field.ty {
            ResolvedTypeRef::Optional(inner) => {
                w.line(&format!("if let Some(__val) = &{accessor} {{"));
                w.indent();
                emit_write_expr(inner, &deref_if_copy(inner, "__val"), None, true, w);
                w.dedent();
                w.line("}");
            }
            _ => emit_write_expr(&field.ty, &accessor, None, false, w),
        }
    }
}

pub(crate) fn emit_write_expr(
    ty: &ResolvedTypeRef,
    accessor: &str,
    vn: Option<&ResolvedSchema>,
    is_ref: bool,
    w: &mut CodeWriter,
) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) => {
            w.line(&format!("{}(__buf, {accessor});", info.write_fn));
        }

        ResolvedTypeRef::Enum(id, width) => {
            let (rust_int, _, write_prim) = width_prim_names(*width);
            if let Some(schema) = vn {
                // if id is the latest enum version every variant is in range,
                // so the clamp branch below is dead, skip it
                let latest_enum_version = schema
                    .enums
                    .enums
                    .keys()
                    .filter(|k| k.name == id.name)
                    .map(|k| k.version)
                    .max();
                if latest_enum_version == Some(id.version) {
                    w.line(&format!("{write_prim}(__buf, {accessor} as {rust_int});"));
                } else {
                    let max = schema
                        .enums
                        .enums
                        .get(id)
                        .and_then(|e| e.variants.iter().map(|v| v.wire_value).max())
                        .unwrap_or(0);
                    w.line(&format!(
                        "{{ let __disc = {accessor} as u32; \
                         {write_prim}(__buf, if __disc <= {max} {{ __disc as {rust_int} }} else {{ 0{rust_int} }}); }}"
                    ));
                }
            } else {
                w.line(&format!("{write_prim}(__buf, {accessor} as {rust_int});"));
            }
        }

        ResolvedTypeRef::Union(id) => {
            // union writers are always fallible: a discriminant too wide for
            // this version's width is a genuine encode error.
            let extra = if is_union_write_intern_infected(id) {
                ", __interner"
            } else {
                ""
            };
            w.line(&format!(
                "write_{}_v{}(__buf, {}{accessor}{extra})?;",
                id.name.to_snake_case(),
                id.version,
                if is_ref { "" } else { "&" }
            ));
        }

        ResolvedTypeRef::Bitset(_, _) => {
            w.line(&format!(
                "{}(__buf, {}{accessor});",
                info.write_fn,
                if is_ref { "" } else { "&" }
            ));
        }

        ResolvedTypeRef::Scalar(id) => {
            let q = if is_constraint_infected(&id.name) {
                "?"
            } else {
                ""
            };
            let extra = if is_intern_infected(&id.name) {
                ", __interner"
            } else {
                ""
            };
            w.line(&format!(
                "{}(__buf, {}{accessor}{extra}){q};",
                info.write_fn,
                if is_ref { "" } else { "&" }
            ));
        }

        ResolvedTypeRef::Array(inner) => {
            if let ResolvedTypeRef::Scalar(id) = inner.as_ref()
                && is_bulk_castable_scalar(&id.name)
            {
                w.line(&format!("write_pod_array(__buf, {accessor}.as_slice());"));
                return;
            }
            w.line(&format!("write_array_len(__buf, {accessor}.len());"));
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(inner, &deref_if_copy(inner, "__item"), vn, true, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::FixedArray(inner, _) => {
            if let ResolvedTypeRef::Scalar(id) = inner.as_ref()
                && is_bulk_castable_scalar(&id.name)
            {
                w.line(&format!(
                    "write_fixed_pod_array(__buf, {}{accessor});",
                    if is_ref { "" } else { "&" }
                ));
                return;
            }
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(inner, &deref_if_copy(inner, "__item"), vn, true, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::DeltaArray(_) => {
            w.line(&format!("write_delta_array(__buf, {accessor}.as_slice());"));
        }

        ResolvedTypeRef::FixedDeltaArray(_, _) => {
            w.line(&format!("write_fixed_delta_array(__buf, &{accessor}[..]);"));
        }

        ResolvedTypeRef::FixedString(_) => {
            w.line(&format!(
                "{}(__buf, {}{accessor});",
                info.write_fn,
                if is_ref { "" } else { "&" }
            ));
        }

        ResolvedTypeRef::Map(k_ty, v_ty) => {
            w.line(&format!("write_array_len(__buf, {accessor}.len());"));
            w.line(&format!("for (__k, __v) in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(k_ty, &deref_if_copy(k_ty, "__k"), vn, true, w);
            emit_write_expr(v_ty, &deref_if_copy(v_ty, "__v"), vn, true, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::FixedMap(k_ty, v_ty, n) => {
            w.line(&format!(
                "assert_eq!({accessor}.inner.len(), {n}, \"FixedMap length mismatch\");"
            ));
            w.line(&format!("for (__k, __v) in {accessor}.inner.iter() {{"));
            w.indent();
            emit_write_expr(k_ty, &deref_if_copy(k_ty, "__k"), vn, true, w);
            emit_write_expr(v_ty, &deref_if_copy(v_ty, "__v"), vn, true, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::Tuple(elements) => {
            for (i, elem) in elements.iter().enumerate() {
                emit_write_expr(elem, &format!("{accessor}.{i}"), vn, false, w);
            }
        }

        ResolvedTypeRef::VFloat {
            min, step, backing, ..
        } => {
            w.line(&format!(
                "{}(__buf, (({accessor} as f64 - {min}f64) / {step}f64).round() as {});",
                info.write_fn,
                backing.rust_int_type()
            ));
        }

        ResolvedTypeRef::Optional(inner) => {
            w.line(&format!("match &{accessor} {{"));
            w.indent();
            w.line("Some(__val) => {");
            w.indent();
            w.line("write_u8(__buf, 1);");
            emit_write_expr(inner, &deref_if_copy(inner, "__val"), vn, true, w);
            w.dedent();
            w.line("}");
            w.line("None => write_u8(__buf, 0),");
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::Boxed(inner) => {
            // Box<T> auto-derefs to &T, so non-Copy inner types need no manual
            // deref (avoids clippy's explicit_auto_deref); Copy primitives still
            // need deref_if_copy's explicit *
            let boxed_ref = if is_ref {
                accessor.to_string()
            } else {
                format!("&{accessor}")
            };
            let derefed = deref_if_copy(inner, &boxed_ref);
            let now_a_value = derefed != boxed_ref;
            emit_write_expr(inner, &derefed, vn, !now_a_value, w);
        }

        // transparent on the wire, constraint check is emitted separately by the caller
        ResolvedTypeRef::Constrained { inner, .. } => {
            emit_write_expr(inner, accessor, vn, is_ref, w)
        }

        // accessor is always already &str here, no dereffing needed regardless of is_ref
        ResolvedTypeRef::Interned(_) => {
            w.line(&format!(
                "write_varint64(__buf, __interner.intern({accessor}) as u64);"
            ));
        }

        ResolvedTypeRef::ImportedSchema {
            alias, root_name, ..
        } => {
            let qualified = format!("{}::{root_name}", alias.to_snake_case());
            let q = if is_constraint_infected(&qualified) {
                "?"
            } else {
                ""
            };
            w.line(&format!(
                "{}(__buf, {}{accessor}){q};",
                info.write_fn,
                if is_ref { "" } else { "&" }
            ));
        }
    }
}

fn emit_encode_vn_fn(schema: &ResolvedSchema, vl: &VersionLineage, lt: &str, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let v = vl.version;
    let fallible = is_constraint_infected(name);
    let ret = if fallible { " -> PojocResult<()>" } else { "" };
    // per-version check, not is_intern_infected(name) — this version's wire
    // format shouldn't gain an intern table just because a later version added one
    let interning = crate::codegen::version_uses_intern(schema, &vl.fields);

    let params = format!("__buf: &mut Vec<u8>, __value: &{name}{lt}");
    w.line(&format!("pub fn encode_v{v}{lt}({params}){ret} {{"));
    if interning {
        w.indent();
        w.line(&format!("encode_interned_v{v}(__buf, __value, &[])"));
        w.dedent();
        w.line("}");
        w.blank();
        let generics = if lt.is_empty() {
            "<const VERSION_LEN: usize>"
        } else {
            "<'buf, const VERSION_LEN: usize>"
        };
        w.line(&format!(
            "fn encode_interned_v{v}{generics}({params}, __version: &[u8; VERSION_LEN]){ret} {{"
        ));
    }
    w.indent();

    if fallible {
        // validated against the current struct, not this version's historical shape
        for field in &schema.versions.last().unwrap().fields {
            emit_constraint_guard(field, &format!("__value.{}", field.name), w);
        }
    }

    if interning {
        // string table must precede fields on wire but needs every intern field
        // visited first, so encode fields into a scratch buffer and prepend the table after
        w.line("let mut __interner_owned = InternBuilder::new();");
        w.line("let __interner = &mut __interner_owned;");
        w.line("let mut __scratch: Vec<u8> = Vec::new();");
        w.line("{");
        w.indent();
        w.line("let __buf = &mut __scratch;");
    }

    // same fixed-first reordering the decoder uses, keeps wire order matching
    let ordered = ordered_wire_fields(vl);
    let opt_fields: Vec<&FieldLineage> = ordered
        .iter()
        .copied()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .collect();
    let header_bytes = opt_fields.len().div_ceil(8);
    if header_bytes > 0 {
        w.line(&format!("let mut __header = [0u8; {header_bytes}];"));
        for (idx, fl) in opt_fields.iter().enumerate() {
            emit_vn_optional_header_bit(schema, fl, idx / 8, idx % 8, w);
        }
        w.line("write_fixed_bytes(__buf, &__header);");
    }

    for &fl in &ordered {
        if let ResolvedTypeRef::Optional(inner_src) = &fl.source_ty {
            emit_vn_optional_body(schema, fl, inner_src, w);
        } else {
            emit_vn_nonoptional_field(schema, fl, w);
        }
    }

    if interning {
        w.dedent();
        w.line("}");
        w.line(
            "write_interned_payload(__buf, __version, &__interner_owned.finish_table(), &__scratch);",
        );
    }

    if fallible {
        w.line("Ok(())");
    }
    w.dedent();
    w.line("}");
}

fn emit_vn_optional_header_bit(
    schema: &ResolvedSchema,
    fl: &FieldLineage,
    byte_idx: usize,
    bit_idx: usize,
    w: &mut CodeWriter,
) {
    match &fl.mapping {
        FieldMapping::Discard => {}
        FieldMapping::PassThrough { target_name } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                w.line(&format!(
                    "if match &__value.{target_name} {{ \
                         LazyView::Raw {{ buf, .. }} => !buf.is_empty(), \
                         LazyView::Owned(__val) => __val.is_some(), \
                     }} {{ __header[{byte_idx}] |= 1 << {bit_idx}; }}"
                ));
            } else {
                w.line(&format!(
                    "if __value.{target_name}.is_some() {{ __header[{byte_idx}] |= 1 << {bit_idx}; }}"
                ));
            }
        }
        FieldMapping::Cast {
            target_name, to, ..
        } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if !target_is_lazy {
                if matches!(to, ResolvedTypeRef::Optional(_)) {
                    w.line(&format!(
                        "if __value.{target_name}.is_some() {{ __header[{byte_idx}] |= 1 << {bit_idx}; }}"
                    ));
                } else {
                    w.line(&format!("__header[{byte_idx}] |= 1 << {bit_idx};"));
                }
            }
        }
    }
}

fn emit_vn_optional_body(
    schema: &ResolvedSchema,
    fl: &FieldLineage,
    inner_src: &ResolvedTypeRef,
    w: &mut CodeWriter,
) {
    match &fl.mapping {
        FieldMapping::Discard => {}
        FieldMapping::PassThrough { target_name } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                emit_lazy_encode(target_name, &fl.source_ty, schema, w);
            } else {
                w.line(&format!("if let Some(__val) = &__value.{target_name} {{"));
                w.indent();
                emit_write_expr(
                    inner_src,
                    &deref_if_copy(inner_src, "__val"),
                    Some(schema),
                    true,
                    w,
                );
                w.dedent();
                w.line("}");
            }
        }
        FieldMapping::Cast {
            target_name,
            from,
            to,
        } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if !target_is_lazy {
                match (from, to) {
                    (ResolvedTypeRef::Optional(f_inner), ResolvedTypeRef::Optional(t_inner)) => {
                        w.line(&format!("if let Some(__val) = &__value.{target_name} {{"));
                        w.indent();
                        emit_vn_cast_value(
                            schema,
                            f_inner,
                            t_inner,
                            &deref_if_copy(t_inner, "__val"),
                            true,
                            w,
                        );
                        w.dedent();
                        w.line("}");
                    }
                    (ResolvedTypeRef::Optional(f_inner), t_inner) => {
                        emit_vn_cast_value(
                            schema,
                            f_inner,
                            t_inner,
                            &format!("__value.{target_name}"),
                            false,
                            w,
                        );
                    }
                    _ => {
                        w.line(&format!("if let Some(__val) = &__value.{target_name} {{"));
                        w.indent();
                        emit_write_expr(
                            inner_src,
                            &deref_if_copy(inner_src, "__val"),
                            Some(schema),
                            true,
                            w,
                        );
                        w.dedent();
                        w.line("}");
                    }
                }
            }
        }
    }
}

fn emit_vn_nonoptional_field(schema: &ResolvedSchema, fl: &FieldLineage, w: &mut CodeWriter) {
    match &fl.mapping {
        FieldMapping::Discard => {
            emit_vn_default_write(&fl.source_ty, schema, w);
        }
        FieldMapping::PassThrough { target_name } => {
            let target_field = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap();
            if target_field.lazy {
                emit_lazy_encode(target_name, &fl.source_ty, schema, w);
            } else {
                emit_write_expr(
                    &fl.source_ty,
                    &format!("__value.{target_name}"),
                    Some(schema),
                    false,
                    w,
                );
            }
        }
        FieldMapping::Cast {
            target_name,
            from,
            to,
        } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                emit_vn_default_write(from, schema, w);
            } else {
                emit_vn_cast_value(
                    schema,
                    from,
                    to,
                    &format!("__value.{target_name}"),
                    false,
                    w,
                );
            }
        }
    }
}

fn emit_vn_cast_value(
    schema: &ResolvedSchema,
    from: &ResolvedTypeRef,
    to: &ResolvedTypeRef,
    accessor: &str,
    is_ref: bool,
    w: &mut CodeWriter,
) {
    use ResolvedTypeRef::*;
    match (from, to) {
        (Scalar(f), Scalar(t)) if is_primitive(&f.name) && is_primitive(&t.name) => {
            let f_norm = normalize_type(&f.name);
            let t_norm = normalize_type(&t.name);
            if f_norm == t_norm || f_norm == "string" || t_norm == "string" {
                emit_write_expr(from, accessor, Some(schema), is_ref, w);
            } else {
                let f_info = type_info(from);
                w.line(&format!(
                    "{}(__buf, {accessor} as {});",
                    f_info.write_fn, f_info.rust_type
                ));
            }
        }

        (Scalar(f_id), Scalar(_)) if !is_primitive(&f_id.name) => {
            w.line("{");
            w.indent();
            if let Some(historical_struct) = schema.types.types.get(f_id) {
                let value_binding = if historical_struct.fields.is_empty() {
                    "_val"
                } else {
                    "val"
                };
                w.line(&format!("let {value_binding} = &{accessor};"));

                let historical_opts: Vec<&FieldIR> = historical_struct
                    .fields
                    .iter()
                    .filter(|f| matches!(f.ty, Optional(_)))
                    .collect();
                let header_bytes = historical_opts.len().div_ceil(8);
                if header_bytes > 0 {
                    w.line(&format!("let mut __nested_header = [0u8; {header_bytes}];"));
                    for (idx, f) in historical_opts.iter().enumerate() {
                        w.line(&format!(
                            "if {value_binding}.{}.is_some() {{ __nested_header[{}] |= 1 << {}; }}",
                            f.name,
                            idx / 8,
                            idx % 8
                        ));
                    }
                    w.line("write_fixed_bytes(__buf, &__nested_header);");
                }

                let current_type_version = schema
                    .types
                    .types
                    .iter()
                    .filter(|(id, _)| id.name == f_id.name)
                    .max_by_key(|(id, _)| id.version)
                    .map(|(_, ir)| ir);

                for field in &historical_struct.fields {
                    let field_exists_currently = current_type_version
                        .map(|curr| curr.fields.iter().any(|f| f.name == field.name))
                        .unwrap_or(false);

                    if field_exists_currently {
                        let field_accessor = format!("{value_binding}.{}", field.name);
                        match &field.ty {
                            Optional(inner) => {
                                w.line(&format!("if let Some(__val) = &{field_accessor} {{"));
                                w.indent();
                                emit_write_expr(
                                    inner,
                                    &deref_if_copy(inner, "__val"),
                                    Some(schema),
                                    true,
                                    w,
                                );
                                w.dedent();
                                w.line("}");
                            }
                            _ => {
                                emit_write_expr(&field.ty, &field_accessor, Some(schema), false, w)
                            }
                        }
                    } else {
                        if !matches!(field.ty, Optional(_)) {
                            emit_vn_default_write(&field.ty, schema, w);
                        }
                    }
                }
            } else {
                panic!(
                    "Historical type definition for {} missing in schema catalog.",
                    f_id.name
                );
            }
            w.dedent();
            w.line("}");
        }

        (Enum(f_id, f_width), Enum(..)) => {
            let (rust_int, _, write_prim) = width_prim_names(*f_width);
            let max = schema
                .enums
                .enums
                .get(f_id)
                .and_then(|e| e.variants.iter().map(|v| v.wire_value).max())
                .unwrap_or(0);
            w.line(&format!(
                "{{ let __disc = {accessor} as u32; \
                 {write_prim}(__buf, if __disc <= {max} {{ __disc as {rust_int} }} else {{ 0{rust_int} }}); }}"
            ));
        }

        (
            VFloat {
                min: f_min,
                step: f_step,
                backing: f_backing,
                ..
            },
            VFloat { .. },
        ) => {
            w.line(&format!(
                "{wfn}(__buf, (({accessor} as f64 - {f_min}f64) / {f_step}f64).round() as {ri});",
                wfn = f_backing.write_fn(),
                ri = f_backing.rust_int_type(),
            ));
        }

        (FixedString(f_n), FixedString(t_n)) => {
            let copy_n = (*f_n).min(*t_n);
            w.line(&format!(
                "{{ let mut __tmp = [0u8; {f_n}]; \
                 __tmp[..{copy_n}].copy_from_slice(&{accessor}[..{copy_n}]); \
                 write_fixed_bytes::<{f_n}>(__buf, &__tmp); }}"
            ));
        }

        (FixedArray(elem_ty, f_n), Map(_, _)) => {
            w.line("{");
            w.indent();
            w.line("let mut __written = 0usize;");
            w.line(&format!("for (_, __v) in {accessor}.iter().take({f_n}) {{"));
            w.indent();
            emit_write_expr(elem_ty, &deref_if_copy(elem_ty, "__v"), None, true, w);
            w.line("__written += 1;");
            w.dedent();
            w.line("}");
            w.line(&format!("for _ in __written..{f_n} {{"));
            w.indent();
            emit_vn_default_write(elem_ty, schema, w);
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
        }

        (FixedArray(elem_ty, f_n), FixedArray(_, t_n)) => {
            if f_n <= t_n {
                w.line(&format!("for __item in {accessor}[..{f_n}].iter() {{"));
                w.indent();
                emit_write_expr(elem_ty, &deref_if_copy(elem_ty, "__item"), None, true, w);
                w.dedent();
                w.line("}");
            } else {
                w.line(&format!("for __item in {accessor}.iter() {{"));
                w.indent();
                emit_write_expr(elem_ty, &deref_if_copy(elem_ty, "__item"), None, true, w);
                w.dedent();
                w.line("}");
                w.line(&format!("for _ in {t_n}..{f_n} {{"));
                w.indent();
                emit_vn_default_write(elem_ty, schema, w);
                w.dedent();
                w.line("}");
            }
        }

        (FixedDeltaArray(elem_ty, f_n), FixedDeltaArray(_, t_n)) => {
            if f_n <= t_n {
                w.line(&format!(
                    "write_fixed_delta_array(__buf, &{accessor}[..{f_n}]);"
                ));
            } else {
                let elem_rust = type_info(elem_ty).rust_type;
                let elem_default = type_info(elem_ty).default_expr;
                w.line("{");
                w.indent();
                w.line(&format!(
                    "let mut __tmp: [{elem_rust}; {f_n}] = [{elem_default}; {f_n}];"
                ));
                w.line(&format!("__tmp[..{t_n}].copy_from_slice(&{accessor}[..]);"));
                w.line("write_fixed_delta_array(__buf, &__tmp[..]);");
                w.dedent();
                w.line("}");
            }
        }

        (Array(_), DeltaArray(_)) => {
            emit_write_expr(from, accessor, Some(schema), is_ref, w);
        }

        (DeltaArray(_), Array(_)) => {
            w.line(&format!("write_delta_array(__buf, {accessor}.as_slice());"));
        }

        (FixedMap(fk, fv, f_n), FixedMap(_, _, _)) => {
            w.line(&format!("for __i in 0usize..{f_n} {{"));
            w.indent();
            w.line(&format!(
                "if let Some((__k, __v)) = {accessor}.inner.get(__i) {{"
            ));
            w.indent();
            emit_write_expr(fk, &deref_if_copy(fk, "__k"), None, true, w);
            emit_write_expr(fv, &deref_if_copy(fv, "__v"), None, true, w);
            w.dedent();
            w.line("} else {");
            w.indent();
            emit_vn_default_write(fk, schema, w);
            emit_vn_default_write(fv, schema, w);
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
        }

        (Bitset(_, width), Scalar(_)) => match width {
            0 => {}
            1 => w.line(&format!("write_u8(__buf, {accessor} as u8);")),
            2 => w.line(&format!("write_u16(__buf, {accessor} as u16);")),
            4 => w.line(&format!("write_u32(__buf, {accessor} as u32);")),
            n => {
                let width = (*n).min(16).next_power_of_two();
                let copy = (*n).min(width);
                let bits = width * 8;
                w.line(&format!("{{ let packed = ({accessor} as u{bits}).to_le_bytes(); \
                                 let mut flags = [0u8; {n}]; flags[..{copy}].copy_from_slice(&packed[..{copy}]); \
                                 write_fixed_bytes(__buf, &flags); }}"));
            }
        },

        (Scalar(f), Bitset(_, width)) if is_primitive(&f.name) => match width {
            1 => w.line(&format!("write_u8(__buf, {accessor}.0[0]);")),
            2 => w.line(&format!(
                "write_u16(__buf, u16::from_le_bytes([{accessor}.0[0], {accessor}.0[1]]));"
            )),
            _ => w.line(&format!(
                "write_u32(__buf, u32::from_le_bytes(\
                     [{accessor}.0[0], {accessor}.0[1], {accessor}.0[2], {accessor}.0[3]]));"
            )),
        },

        (from_ty, Optional(t_inner)) => {
            w.line(&format!("match &{accessor} {{"));
            w.indent();
            w.line("Some(__val) => {");
            w.indent();
            emit_vn_cast_value(
                schema,
                from_ty,
                t_inner,
                &deref_if_copy(t_inner, "__val"),
                true,
                w,
            );
            w.dedent();
            w.line("}");
            w.line("None => {");
            w.indent();
            emit_vn_default_write(from_ty, schema, w);
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
        }

        (Boxed(f_inner), Boxed(t_inner)) => {
            // unwrap via auto-deref like emit_write_expr's Boxed arm, not manual &**
            let boxed_ref = if is_ref {
                accessor.to_string()
            } else {
                format!("&{accessor}")
            };
            let derefed = deref_if_copy(t_inner, &boxed_ref);
            let now_a_value = derefed != boxed_ref;
            emit_vn_cast_value(schema, f_inner, t_inner, &derefed, !now_a_value, w);
        }

        (from_ty, Boxed(t_inner)) if !matches!(from_ty, Boxed(_)) => {
            let boxed_ref = if is_ref {
                accessor.to_string()
            } else {
                format!("&{accessor}")
            };
            let derefed = deref_if_copy(t_inner, &boxed_ref);
            let now_a_value = derefed != boxed_ref;
            emit_vn_cast_value(schema, from_ty, t_inner, &derefed, !now_a_value, w);
        }

        (Boxed(f_inner), to_ty) if !matches!(to_ty, Boxed(_)) => {
            emit_vn_cast_value(schema, f_inner, to_ty, accessor, is_ref, w);
        }

        _ => emit_write_expr(from, accessor, Some(schema), is_ref, w),
    }
}

fn emit_vn_default_write(ty: &ResolvedTypeRef, schema: &ResolvedSchema, w: &mut CodeWriter) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) => {
            w.line(&format!("{}(__buf, {});", info.write_fn, info.default_expr));
        }
        ResolvedTypeRef::Scalar(id) => {
            w.line("{");
            w.indent();
            if let Some(historical_struct) = schema.types.types.get(id) {
                let historical_opts_count = historical_struct
                    .fields
                    .iter()
                    .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
                    .count();
                let header_bytes = historical_opts_count.div_ceil(8);
                if header_bytes > 0 {
                    w.line(&format!(
                        "write_fixed_bytes(__buf, &[0u8; {header_bytes}]);"
                    ));
                }

                for field in &historical_struct.fields {
                    if !matches!(field.ty, ResolvedTypeRef::Optional(_)) {
                        emit_vn_default_write(&field.ty, schema, w);
                    }
                }
            } else {
                w.line(&format!(
                    "write_{}(__buf, &{}::default());",
                    id.name.to_snake_case(),
                    id.name
                ));
            }
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::Enum(_, width) => {
            let (rust_int, _, write_prim) = width_prim_names(*width);
            w.line(&format!("{write_prim}(__buf, 0{rust_int});"));
        }
        ResolvedTypeRef::Union(id) => {
            // 0 always fits any width, so no overflow check needed here
            let width = schema
                .unions
                .unions
                .get(id)
                .map(|u| u.discriminant_width())
                .unwrap_or(1);
            let (rust_int, _, write_prim) = width_prim_names(width);
            w.line(&format!("{write_prim}(__buf, 0{rust_int});"));
            w.line("write_varint32(__buf, 0u32);");
        }
        ResolvedTypeRef::Bitset(id, _) => {
            w.line(&format!(
                "write_{}_v{}(__buf, &{}::default());",
                id.name.to_snake_case(),
                id.version,
                id.name
            ));
        }
        ResolvedTypeRef::Array(_) => {
            w.line("write_array_len(__buf, 0);");
        }
        ResolvedTypeRef::DeltaArray(_) => {
            w.line("write_varint32(__buf, 0u32);");
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            if *n > 0 {
                w.line(&format!("for _ in 0..{n} {{"));
                w.indent();
                emit_vn_default_write(inner, schema, w);
                w.dedent();
                w.line("}");
            }
        }
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            let inner_rust = type_info(inner).rust_type;
            let inner_default = type_info(inner).default_expr;
            w.line(&format!(
                "write_fixed_delta_array(__buf, &[{inner_rust}::default(); {n}][..]);"
            ));
            let _ = inner_default;
        }
        ResolvedTypeRef::FixedString(n) => {
            w.line(&format!("write_fixed_bytes::<{n}>(__buf, &[0u8; {n}]);"));
        }
        ResolvedTypeRef::Map(_, _) => {
            w.line("write_array_len(__buf, 0);");
        }
        ResolvedTypeRef::FixedMap(k, v, n) => {
            if *n > 0 {
                w.line(&format!("for _ in 0..{n} {{"));
                w.indent();
                emit_vn_default_write(k, schema, w);
                emit_vn_default_write(v, schema, w);
                w.dedent();
                w.line("}");
            }
        }
        ResolvedTypeRef::Tuple(elements) => {
            for elem in elements {
                emit_vn_default_write(elem, schema, w);
            }
        }
        ResolvedTypeRef::VFloat { backing, .. } => {
            w.line(&format!(
                "{}(__buf, 0{});",
                backing.write_fn(),
                backing.rust_int_type()
            ));
        }
        ResolvedTypeRef::Optional(_) => {}
        ResolvedTypeRef::Boxed(inner) => {
            // box<T>'s wire default is just T's default
            emit_vn_default_write(inner, schema, w);
        }
        ResolvedTypeRef::Constrained { inner, .. } => emit_vn_default_write(inner, schema, w),
        // old-version reader expects a table index here, filler must go through the interner
        ResolvedTypeRef::Interned(_) => {
            w.line("write_varint64(__buf, __interner.intern(\"\") as u64);");
        }
        ResolvedTypeRef::ImportedSchema { .. } => {
            w.line(&format!(
                "{}(__buf, &{}::default());",
                info.write_fn, info.rust_type
            ));
        }
    }
}

fn emit_type_size_hints(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = latest_by_name(&schema.types.types);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();
    for name in names {
        let (_, resolved) = latest[name];
        let lt = if is_infected(name) { "<'buf>" } else { "" };
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn size_hint_{}{lt}(__value: &{name}{lt}) -> usize {{",
            name.to_snake_case()
        ));
        w.indent();
        if resolved.fields.is_empty() {
            w.line("0usize");
        } else {
            w.line("let mut size = 0usize;");
            emit_optional_header_size(&resolved.fields, w);
            emit_fields_size_loop(&resolved.fields, w, schema);
            w.line("size");
        }
        w.dedent();
        w.line("}");
        w.blank();
    }
}

// mirrors emit_union_writers: one size hint per resolved union version.
fn emit_union_size_hints(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest_by_name = latest_by_name(&schema.unions.unions);

    let mut entries: Vec<(&TypeId, &ResolvedUnion)> = schema.unions.unions.iter().collect();
    entries
        .sort_by(|(a, _), (b, _)| (a.name.as_str(), a.version).cmp(&(b.name.as_str(), b.version)));

    for (id, resolved) in entries {
        let name = &id.name;
        let (_, latest) = latest_by_name[name];
        let width = resolved.discriminant_width();
        let max_value = width_max_value(width);
        let lt = if is_infected(name) { "<'buf>" } else { "" };
        let fn_name = format!("size_hint_{}_v{}", name.to_snake_case(), id.version);
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}{lt}(__value: &{name}{lt}) -> usize {{"
        ));
        w.indent();
        w.line("match __value {");
        w.indent();
        for variant in &latest.variants {
            if variant.discriminant > max_value {
                w.line(&format!("{name}::{}(_) => 0,", variant.name));
                continue;
            }
            w.line(&format!("{name}::{}(__p) => {{", variant.name));
            w.indent();
            match &variant.payload {
                ResolvedTypeRef::Scalar(pid) if !is_primitive(&pid.name) => {
                    let size_fn = format!("size_hint_{}", pid.name.to_snake_case());
                    w.line(&format!("let __plen = {size_fn}(__p);"));
                }
                _ => {
                    w.line("let mut size = 0usize;");
                    emit_size_expr(&variant.payload, "__p", true, w, schema);
                    w.line("let __plen = size;");
                }
            }
            w.line(&format!("{width} + varint_size(__plen) + __plen"));
            w.dedent();
            w.line("}");
        }
        // same overflow-guard condition as emit_union_writers.
        let unknown_size = format!("{width} + varint_size(data.len()) + data.len()");
        let unknown_arm = if width < latest.discriminant_width() {
            format!(
                "{name}::Unknown {{ discriminant, data }} => if *discriminant > {max_value} {{ 0 }} else {{ {unknown_size} }},"
            )
        } else {
            format!("{name}::Unknown {{ data, .. }} => {unknown_size},")
        };
        w.line(&unknown_arm);
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_optional_header_size(fields: &[FieldIR], w: &mut CodeWriter) {
    let n = fields
        .iter()
        .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
        .count();
    if n > 0 {
        w.line(&format!("size += {};", n.div_ceil(8)));
    }
}

fn emit_fields_size_loop(fields: &[FieldIR], w: &mut CodeWriter, schema: &ResolvedSchema) {
    for field in fields {
        if field.lazy {
            emit_lazy_size(&field.name, &field.ty, schema, w);
            continue;
        }
        let accessor = format!("__value.{}", field.name);
        match &field.ty {
            ResolvedTypeRef::Optional(inner) => {
                w.line(&format!("if let Some(__val) = &{accessor} {{"));
                w.indent();
                emit_size_expr(inner, "__val", true, w, schema);
                w.dedent();
                w.line("}");
            }
            _ => emit_size_expr(&field.ty, &accessor, false, w, schema),
        }
    }
}

fn emit_size_expr(
    ty: &ResolvedTypeRef,
    accessor: &str,
    is_ref: bool,
    w: &mut CodeWriter,
    schema: &ResolvedSchema,
) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(_) => {
            w.line(&format!("size += {};", info.size_expr(is_ref, accessor)));
        }
        ResolvedTypeRef::Enum(_, width) => {
            w.line(&format!("size += {width};"));
        }
        ResolvedTypeRef::Union(id) => {
            // info.size_expr's generic rust_type-derived fallback doesn't know
            // about the per-version function-name suffix (see type_info's
            // Union arm), so compute the call directly here instead
            let borrow_symbol = if is_ref { "" } else { "&" };
            w.line(&format!(
                "size += size_hint_{}_v{}({borrow_symbol}{accessor});",
                id.name.to_snake_case(),
                id.version
            ));
        }
        ResolvedTypeRef::Bitset(id, _) => {
            let width = schema
                .bitsets
                .bitsets
                .get(id)
                .map_or(1, ResolvedBitset::byte_width);
            w.line(&format!("size += {width};"));
        }
        ResolvedTypeRef::Array(inner) => {
            if let ResolvedTypeRef::Scalar(id) = inner.as_ref()
                && is_bulk_castable_scalar(&id.name)
                && let WireSize::Fixed(elem_size) = type_info(inner).wire_size
            {
                let bytes = if elem_size == 1 {
                    format!("{accessor}.len()")
                } else {
                    format!("{accessor}.len() * {elem_size}")
                };
                w.line(&format!("size += varint_size({accessor}.len()) + {bytes};"));
                return;
            }
            w.line(&format!("size += varint_size({accessor}.len());"));
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            emit_size_expr(inner, "__item", true, w, schema);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            let inner_info = type_info(inner);
            if let WireSize::Fixed(elem_size) = inner_info.wire_size {
                if *n > 0 {
                    let size_str = if *n == 1 {
                        format!("size += {elem_size};")
                    } else if elem_size == 1 {
                        format!("size += {n};")
                    } else {
                        format!("size += {n} * {elem_size};")
                    };
                    w.line(&size_str);
                }
            } else if *n > 0 {
                w.line(&format!("for __item in {accessor}.iter() {{"));
                w.indent();
                emit_size_expr(inner, "__item", true, w, schema);
                w.dedent();
                w.line("}");
            }
        }
        ResolvedTypeRef::DeltaArray(_) => {
            w.line(&format!(
                "size += delta_array_size_hint({accessor}.as_slice());"
            ));
        }
        ResolvedTypeRef::FixedDeltaArray(_, _) => {
            w.line(&format!(
                "size += fixed_delta_array_size_hint(&{accessor}[..]);"
            ));
        }
        ResolvedTypeRef::FixedString(n) => {
            w.line(&format!("size += {n};"));
        }
        ResolvedTypeRef::Map(k_ty, v_ty) => {
            // fixed-size side folds into len() * size, only iterate the varying side(s)
            let k_wire = type_info(k_ty).wire_size;
            let v_wire = type_info(v_ty).wire_size;
            match (k_wire, v_wire) {
                (WireSize::Fixed(ks), WireSize::Fixed(vs)) => {
                    w.line(&format!("size += {accessor}.len() * ({ks} + {vs});"));
                }
                (WireSize::Fixed(ks), WireSize::Variable) => {
                    let bytes = if ks == 1 {
                        format!("{accessor}.len()")
                    } else {
                        format!("{accessor}.len() * {ks}")
                    };
                    w.line(&format!("size += {bytes};"));
                    w.line(&format!("for __v in {accessor}.values() {{"));
                    w.indent();
                    emit_size_expr(v_ty, "__v", true, w, schema);
                    w.dedent();
                    w.line("}");
                }
                (WireSize::Variable, WireSize::Fixed(vs)) => {
                    let bytes = if vs == 1 {
                        format!("{accessor}.len()")
                    } else {
                        format!("{accessor}.len() * {vs}")
                    };
                    w.line(&format!("size += {bytes};"));
                    w.line(&format!("for __k in {accessor}.keys() {{"));
                    w.indent();
                    emit_size_expr(k_ty, "__k", true, w, schema);
                    w.dedent();
                    w.line("}");
                }
                (WireSize::Variable, WireSize::Variable) => {
                    w.line(&format!("for (__k, __v) in {accessor}.iter() {{"));
                    w.indent();
                    emit_size_expr(k_ty, "__k", true, w, schema);
                    emit_size_expr(v_ty, "__v", true, w, schema);
                    w.dedent();
                    w.line("}");
                }
            }
        }
        ResolvedTypeRef::FixedMap(k_ty, v_ty, n) => {
            let k_info = type_info(k_ty);
            let v_info = type_info(v_ty);
            if let (WireSize::Fixed(ks), WireSize::Fixed(vs)) =
                (&k_info.wire_size, &v_info.wire_size)
            {
                if *n > 0 {
                    let entry_size = ks + vs;
                    let size_str = if *n == 1 {
                        format!("size += {entry_size};")
                    } else {
                        format!("size += {n} * ({ks} + {vs});")
                    };
                    w.line(&size_str);
                }
            } else {
                let k_rust = k_info.rust_type;
                let v_rust = v_info.rust_type;
                w.line(&format!("for __i in 0..{n} {{"));
                w.indent();
                w.line(&format!(
                    "let __default: ({k_rust}, {v_rust}) = Default::default();"
                ));
                w.line(&format!(
                    "let (__k, __v) = {accessor}.inner.get(__i).unwrap_or(&__default);"
                ));
                emit_size_expr(k_ty, "__k", true, w, schema);
                emit_size_expr(v_ty, "__v", true, w, schema);
                w.dedent();
                w.line("}");
            }
        }
        ResolvedTypeRef::Tuple(elements) => {
            for (i, elem) in elements.iter().enumerate() {
                emit_size_expr(elem, &format!("{accessor}.{i}"), false, w, schema);
            }
        }
        ResolvedTypeRef::VFloat { .. } => {
            w.line(&format!("size += {};", info.size_expr(is_ref, accessor)));
        }
        ResolvedTypeRef::Optional(inner) => {
            w.line("size += 1;");
            w.line(&format!("if let Some(__val) = {accessor}.as_ref() {{"));
            w.indent();
            emit_size_expr(inner, "__val", true, w, schema);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::Boxed(inner) => {
            // same auto-deref reasoning as emit_write_expr's Boxed arm
            let boxed_ref = if is_ref {
                accessor.to_string()
            } else {
                format!("&{accessor}")
            };
            let derefed = deref_if_copy(inner, &boxed_ref);
            let now_a_value = derefed != boxed_ref;
            emit_size_expr(inner, &derefed, !now_a_value, w, schema);
        }
        ResolvedTypeRef::Constrained { inner, .. } => {
            emit_size_expr(inner, accessor, is_ref, w, schema)
        }
        // wire cost is a small varint index, not the string; flat estimate is fine
        // since this only drives Vec::reserve, not correctness
        ResolvedTypeRef::Interned(_) => {
            w.line("size += 10;");
        }
        ResolvedTypeRef::ImportedSchema { alias, version, .. } => {
            let module = alias.to_snake_case();
            let ref_prefix = if is_ref { "" } else { "&" };
            w.line(&format!(
                "size += {module}::size_hint_v{version}({ref_prefix}{accessor});"
            ));
        }
    }
}

fn emit_size_hint_vn_fn(
    schema: &ResolvedSchema,
    vl: &VersionLineage,
    lt: &str,
    w: &mut CodeWriter,
) {
    let name = &schema.name_hint;
    let v = vl.version;

    w.line(&format!(
        "pub fn size_hint_v{v}{lt}(__value: &{name}{lt}) -> usize {{"
    ));
    w.indent();

    let opt_count = vl
        .fields
        .iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .count();
    let header_bytes = opt_count.div_ceil(8);

    if vl.fields.is_empty() {
        w.line("0usize");
    } else {
        w.line("let mut size = 0usize;");
        if header_bytes > 0 {
            w.line(&format!("size += {header_bytes};"));
        }
        for fl in &vl.fields {
            emit_vn_field_size_hint(schema, fl, w);
        }
        w.line("size");
    }

    w.dedent();
    w.line("}");
}

fn emit_vn_field_size_hint(schema: &ResolvedSchema, fl: &FieldLineage, w: &mut CodeWriter) {
    match &fl.source_ty {
        ResolvedTypeRef::Optional(inner_src) => {
            emit_vn_optional_field_size_hint(schema, fl, inner_src, w);
        }
        _ => emit_vn_nonoptional_field_size_hint(schema, fl, w),
    }
}

fn emit_vn_nonoptional_field_size_hint(
    schema: &ResolvedSchema,
    fl: &FieldLineage,
    w: &mut CodeWriter,
) {
    match &fl.mapping {
        FieldMapping::Discard => emit_default_size(schema, &fl.source_ty, w),
        FieldMapping::PassThrough { target_name } => {
            let target_field = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap();
            if target_field.lazy {
                emit_lazy_size(target_name, &fl.source_ty, schema, w);
            } else {
                let accessor = format!("__value.{target_name}");
                if matches!(target_field.ty, ResolvedTypeRef::Optional(_))
                    && !matches!(fl.source_ty, ResolvedTypeRef::Optional(_))
                {
                    w.line(&format!("if let Some(__val) = &{accessor} {{"));
                    w.indent();
                    emit_size_expr(&fl.source_ty, "__val", true, w, schema);
                    w.dedent();
                    w.line("}");
                } else {
                    emit_size_expr(&fl.source_ty, &accessor, false, w, schema);
                }
            }
        }
        FieldMapping::Cast {
            target_name,
            from,
            to,
        } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                emit_default_size(schema, from, w);
            } else {
                let (f_inner, t_inner) = match (from, to) {
                    (ResolvedTypeRef::Optional(f), ResolvedTypeRef::Optional(t)) => {
                        (f.as_ref(), t.as_ref())
                    }
                    (ResolvedTypeRef::Optional(f), t) => (f.as_ref(), t),
                    (f, ResolvedTypeRef::Optional(t)) => (f, t.as_ref()),
                    (f, t) => (f, t),
                };
                if matches!(from, ResolvedTypeRef::Optional(_))
                    && matches!(to, ResolvedTypeRef::Optional(_))
                {
                    w.line(&format!("if let Some(__val) = &__value.{target_name} {{"));
                    w.indent();
                    emit_vn_cast_size_hint(schema, f_inner, t_inner, "__val", true, w);
                    w.dedent();
                    w.line("}");
                } else if matches!(from, ResolvedTypeRef::Optional(_)) {
                    emit_vn_cast_size_hint(
                        schema,
                        f_inner,
                        t_inner,
                        &format!("__value.{target_name}"),
                        false,
                        w,
                    );
                } else if matches!(to, ResolvedTypeRef::Optional(_)) {
                    w.line(&format!("if let Some(__val) = &__value.{target_name} {{"));
                    w.indent();
                    emit_vn_cast_size_hint(schema, f_inner, t_inner, "__val", true, w);
                    w.dedent();
                    w.line("}");
                } else {
                    emit_vn_cast_size_hint(
                        schema,
                        f_inner,
                        t_inner,
                        &format!("__value.{target_name}"),
                        false,
                        w,
                    );
                }
            }
        }
    }
}

fn emit_vn_optional_field_size_hint(
    schema: &ResolvedSchema,
    fl: &FieldLineage,
    inner_src: &ResolvedTypeRef,
    w: &mut CodeWriter,
) {
    match &fl.mapping {
        FieldMapping::Discard => {}
        FieldMapping::PassThrough { target_name } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if target_is_lazy {
                emit_lazy_size(target_name, &fl.source_ty, schema, w);
            } else {
                w.line(&format!("if let Some(__val) = &__value.{target_name} {{"));
                w.indent();
                emit_size_expr(inner_src, "__val", true, w, schema);
                w.dedent();
                w.line("}");
            }
        }
        FieldMapping::Cast {
            target_name,
            from,
            to,
        } => {
            let target_is_lazy = schema
                .versions
                .last()
                .unwrap()
                .fields
                .iter()
                .find(|f| f.name == *target_name)
                .unwrap()
                .lazy;
            if !target_is_lazy {
                let (f_inner, t_inner) = match (from, to) {
                    (ResolvedTypeRef::Optional(f), ResolvedTypeRef::Optional(t)) => {
                        (f.as_ref(), t.as_ref())
                    }
                    (ResolvedTypeRef::Optional(f), t) => (f.as_ref(), t),
                    (f, ResolvedTypeRef::Optional(t)) => (f, t.as_ref()),
                    (f, t) => (f, t),
                };
                if matches!(to, ResolvedTypeRef::Optional(_)) {
                    w.line(&format!("if let Some(__val) = &__value.{target_name} {{"));
                    w.indent();
                    emit_vn_cast_size_hint(schema, f_inner, t_inner, "__val", true, w);
                    w.dedent();
                    w.line("}");
                } else {
                    emit_vn_cast_size_hint(
                        schema,
                        f_inner,
                        t_inner,
                        &format!("__value.{target_name}"),
                        false,
                        w,
                    );
                }
            }
        }
    }
}

fn emit_vn_cast_size_hint(
    schema: &ResolvedSchema,
    from: &ResolvedTypeRef,
    to: &ResolvedTypeRef,
    accessor: &str,
    is_ref: bool,
    w: &mut CodeWriter,
) {
    use ResolvedTypeRef::*;
    match (from, to) {
        (FixedArray(elem, f_n), FixedArray(..)) => {
            if let WireSize::Fixed(s) = type_info(elem).wire_size {
                if *f_n == 1 {
                    w.line(&format!("size += {s};"))
                } else {
                    if s == 1 {
                        w.line(&format!("size += {f_n};"))
                    } else {
                        w.line(&format!("size += {f_n} * {s};"))
                    }
                }
            } else {
                emit_size_expr(from, accessor, is_ref, w, schema);
            }
        }
        (FixedDeltaArray(_, f_n), FixedDeltaArray(..)) => {
            w.line(&format!(
                "size += fixed_delta_array_size_hint(&{accessor}[..{f_n}]);"
            ));
        }
        (FixedArray(elem, n), Map(_, _)) => {
            // mirrors encode_vN: writes exactly N values, so size walks the same window.
            if let WireSize::Fixed(s) = type_info(elem).wire_size {
                if *n == 1 {
                    w.line(&format!("size += {s};"));
                } else if s == 1 {
                    w.line(&format!("size += {n};"));
                } else {
                    w.line(&format!("size += {n} * {s};"));
                }
            } else {
                w.line("let mut __written = 0usize;");
                w.line(&format!("for (_, __v) in {accessor}.iter().take({n}) {{"));
                w.indent();
                emit_size_expr(elem, "__v", true, w, schema);
                w.line("__written += 1;");
                w.dedent();
                w.line("}");
                w.line(&format!("for _ in __written..{n} {{"));
                w.indent();
                emit_default_size(schema, elem, w);
                w.dedent();
                w.line("}");
            }
        }
        _ => emit_size_expr(from, accessor, is_ref, w, schema),
    }
}

fn emit_default_size(schema: &ResolvedSchema, ty: &ResolvedTypeRef, w: &mut CodeWriter) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) => {
            if let WireSize::Fixed(n) = info.wire_size {
                w.line(&format!("size += {n};"));
            } else {
                w.line("size += 1;");
            }
        }
        ResolvedTypeRef::Scalar(id) => {
            if let Some(hist) = schema.types.types.get(id) {
                let opt_count = hist
                    .fields
                    .iter()
                    .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
                    .count();
                let hb = opt_count.div_ceil(8);
                if hb > 0 {
                    w.line(&format!("size += {hb};"));
                }
                for field in &hist.fields {
                    if !matches!(field.ty, ResolvedTypeRef::Optional(_)) {
                        emit_default_size(schema, &field.ty, w);
                    }
                }
            }
        }
        ResolvedTypeRef::Enum(_, width) => w.line(&format!("size += {width};")),
        ResolvedTypeRef::Union(id) => {
            let width = schema
                .unions
                .unions
                .get(id)
                .map(|u| u.discriminant_width())
                .unwrap_or(1);
            // discriminant (fixed width) + a zero-length payload's 1-byte varint length prefix
            w.line(&format!("size += {};", width + 1));
        }
        ResolvedTypeRef::Bitset(id, _) => {
            let width = schema
                .bitsets
                .bitsets
                .get(id)
                .map_or(1, ResolvedBitset::byte_width);
            w.line(&format!("size += {width};"));
        }
        ResolvedTypeRef::Array(_) | ResolvedTypeRef::Map(_, _) => w.line("size += 1;"),
        ResolvedTypeRef::DeltaArray(_) => w.line("size += 1;"),
        ResolvedTypeRef::FixedArray(inner, n) => {
            if *n > 0 {
                if let WireSize::Fixed(s) = type_info(inner).wire_size {
                    let size_str = if *n == 1 {
                        format!("size += {s};")
                    } else if s == 1 {
                        format!("size += {n};")
                    } else {
                        format!("size += {n} * {s};")
                    };
                    w.line(&size_str);
                } else {
                    for _ in 0..*n {
                        emit_default_size(schema, inner, w);
                    }
                }
            }
        }
        ResolvedTypeRef::FixedDeltaArray(_, n) => {
            if *n > 0 {
                w.line(&format!("size += {n};"));
            }
        }
        ResolvedTypeRef::FixedString(n) => w.line(&format!("size += {n};")),
        ResolvedTypeRef::FixedMap(k, v, n) => {
            if *n > 0 {
                if let (WireSize::Fixed(ks), WireSize::Fixed(vs)) =
                    (type_info(k).wire_size, type_info(v).wire_size)
                {
                    if *n > 0 {
                        let entry_size = ks + vs;
                        let size_str = if *n == 1 {
                            format!("size += {entry_size};")
                        } else {
                            format!("size += {n} * ({ks} + {vs});")
                        };
                        w.line(&size_str);
                    }
                } else {
                    for _ in 0..*n {
                        emit_default_size(schema, k, w);
                        emit_default_size(schema, v, w);
                    }
                }
            }
        }
        ResolvedTypeRef::Tuple(elements) => {
            for elem in elements {
                emit_default_size(schema, elem, w);
            }
        }
        ResolvedTypeRef::VFloat { .. } => {
            if let WireSize::Fixed(n) = info.wire_size {
                w.line(&format!("size += {n};"));
            }
        }
        ResolvedTypeRef::Boxed(inner) => emit_default_size(schema, inner, w),
        ResolvedTypeRef::Constrained { inner, .. } => emit_default_size(schema, inner, w),
        ResolvedTypeRef::Interned(_) => w.line("size += 10;"),
        ResolvedTypeRef::Optional(_) | ResolvedTypeRef::ImportedSchema { .. } => {}
    }
}

fn emit_lazy_encode(
    field_name: &str,
    wire_ty: &ResolvedTypeRef,
    schema: &ResolvedSchema,
    w: &mut CodeWriter,
) {
    let is_optional = matches!(wire_ty, ResolvedTypeRef::Optional(_));
    let inner_ty = match wire_ty {
        ResolvedTypeRef::Optional(i) => i.as_ref(),
        t => t,
    };

    w.line(&format!("match &__value.{field_name} {{"));
    w.indent();
    w.line("LazyView::Raw { buf, .. } if !buf.is_empty() => __buf.extend_from_slice(buf),");
    if !is_optional {
        w.line("LazyView::Raw { .. } => {");
        w.indent();
        emit_vn_default_write(wire_ty, schema, w);
        w.dedent();
        w.line("}");
    } else {
        w.line("LazyView::Raw { .. } => {}");
    }

    if is_optional {
        w.line("LazyView::Owned(__val) => {");
        w.indent();
        w.line("if let Some(__inner) = __val {");
        w.indent();
        match inner_ty {
            ResolvedTypeRef::DeltaArray(_) => w.line("write_delta_array(__buf, __inner);"),
            ResolvedTypeRef::FixedDeltaArray(_, _) => {
                w.line("write_fixed_delta_array(__buf, __inner);")
            }
            _ => emit_write_expr(inner_ty, &deref_if_copy(inner_ty, "__inner"), None, true, w),
        }
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    } else {
        w.line("LazyView::Owned(__val) => {");
        w.indent();
        match inner_ty {
            ResolvedTypeRef::DeltaArray(_) => w.line("write_delta_array(__buf, &**__val);"),
            ResolvedTypeRef::FixedDeltaArray(_, _) => {
                w.line("write_fixed_delta_array(__buf, &**__val);")
            }
            _ => emit_write_expr(inner_ty, &deref_if_copy(inner_ty, "__val"), None, true, w),
        }
        w.dedent();
        w.line("}");
    }

    w.dedent();
    w.line("}");
}

fn emit_lazy_size(
    field_name: &str,
    wire_ty: &ResolvedTypeRef,
    schema: &ResolvedSchema,
    w: &mut CodeWriter,
) {
    let is_optional = matches!(wire_ty, ResolvedTypeRef::Optional(_));
    let inner_ty = match wire_ty {
        ResolvedTypeRef::Optional(i) => i.as_ref(),
        t => t,
    };

    w.line(&format!("match &__value.{field_name} {{"));
    w.indent();
    if is_optional {
        w.line("LazyView::Raw { buf, .. } => size += buf.len(),");
    } else {
        w.line("LazyView::Raw { buf, .. } if !buf.is_empty() => size += buf.len(),");
        w.line("LazyView::Raw { .. } => size += 16,");
    }

    if is_optional {
        w.line("LazyView::Owned(__val) => {");
        w.indent();
        w.line("if let Some(__inner) = __val {");
        w.indent();
        match inner_ty {
            ResolvedTypeRef::DeltaArray(_) => w.line("size += delta_array_size_hint(__inner);"),
            ResolvedTypeRef::FixedDeltaArray(_, _) => {
                w.line("size += fixed_delta_array_size_hint(__inner);")
            }
            _ => emit_size_expr(inner_ty, "__inner", true, w, schema),
        }
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    } else {
        w.line("LazyView::Owned(__val) => {");
        w.indent();
        match inner_ty {
            ResolvedTypeRef::DeltaArray(_) => w.line("size += delta_array_size_hint(&**__val);"),
            ResolvedTypeRef::FixedDeltaArray(_, _) => {
                w.line("size += fixed_delta_array_size_hint(&**__val);")
            }
            _ => emit_size_expr(inner_ty, "__val", true, w, schema),
        }
        w.dedent();
        w.line("}");
    }

    w.dedent();
    w.line("}");
}
