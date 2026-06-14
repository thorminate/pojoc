use super::writer::CodeWriter;
use crate::{emit_default, get_latest_versions};
use pojoc_core::types::*;
use pojoc_schema::ir::lineage::*;
use pojoc_schema::ir::types::*;
use std::collections::{HashMap, HashSet};
use heck::ToSnakeCase;

pub fn emit_decode_functions(schema: &ResolvedSchema, w: &mut CodeWriter) {
    emit_type_readers(schema, w);
    emit_type_skippers(schema, w);
    emit_bitset_readers(schema, w);

    for vl in &schema.lineage.versions {
        emit_decode_fn(schema, vl, w);
        w.blank();
    }
}

fn emit_type_readers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(
        &schema.types.types,
        |id| { id.name.clone() },
        |id| { id.version }
    );

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let buf_param = if resolved.fields.is_empty() { "_buf" } else { "buf" };
        let pos_param = if resolved.fields.is_empty() { "_pos" } else { "pos" };
        let fn_name = format!("read_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}({buf_param}: &[u8], {pos_param}: &mut usize) -> Result<{name}> {{"
        ));
        w.indent();

        let optional_count = resolved.fields.iter().filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_))).count();
        emit_optional_header_read(optional_count, w);

        let mut optional_counter = 0;
        for field in &resolved.fields {
            match &field.ty {
                ResolvedTypeRef::Optional(inner) => {
                    let byte_idx = optional_counter / 8;
                    let bit_idx = optional_counter % 8;
                    optional_counter += 1;
                    let inner_expr = emit_read_expr(inner);
                    w.line(&format!(
                        "let {} = if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{ Some({inner_expr}) }} else {{ None }};",
                        field.name
                    ));
                }
                _ => {
                    let expr = emit_read_expr(&field.ty);
                    w.line(&format!("let {} = {expr};", field.name));
                }
            }
        }

        w.line(&format!("Ok({name} {{"));
        w.indent();
        for field in &resolved.fields {
            w.line(&format!("{},", field.name));
        }
        w.dedent();
        w.line("})");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_type_skippers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(
        &schema.types.types,
        |id| { id.name.clone() },
        |id| { id.version }
    );


    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let buf_param = if resolved.fields.is_empty() { "_buf" } else { "buf" };
        let pos_param = if resolved.fields.is_empty() { "_pos" } else { "pos" };
        let fn_name = format!("skip_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}({buf_param}: &[u8], {pos_param}: &mut usize) -> Result<()> {{"
        ));
        w.indent();

        let optional_count = resolved.fields.iter().filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_))).count();
        emit_optional_header_read(optional_count, w);

        let mut optional_counter = 0;
        for field in &resolved.fields {
            match &field.ty {
                ResolvedTypeRef::Optional(inner) => {
                    let byte_idx = optional_counter / 8;
                    let bit_idx = optional_counter % 8;
                    optional_counter += 1;
                    w.line(&format!("if (__header[{byte_idx}] & (1 << {bit_idx})) != 0 {{"));
                    w.indent();
                    w.line(&emit_skip_stmt(inner));
                    w.dedent();
                    w.line("}");
                }
                _ => {
                    w.line(&emit_skip_stmt(&field.ty));
                }
            }
        }

        w.line("Ok(())");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_bitset_readers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(
        &schema.bitsets.bitsets,
        |id| { id.name.clone() },
        |id| { id.version }
    );


    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, bs) = latest[name];
        let computed_len = (bs.variants.len() + 7) / 8;
        let fn_name = format!("read_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(buf: &[u8], pos: &mut usize) -> Result<{name}> {{"
        ));
        w.indent();
        w.line(&format!(
            "let bytes = read_fixed_bytes::<{computed_len}>(buf, pos)?;"
        ));
        w.line(&format!("Ok({name}(bytes))"));
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_skip_stmt(ty: &ResolvedTypeRef) -> String {
    type_info(ty).skip_stmt
}

fn emit_decode_fn(schema: &ResolvedSchema, vl: &VersionLineage, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let v = vl.version;

    w.line(&format!(
        "pub fn decode_v{v}(buf: &[u8], pos: &mut usize) -> Result<{name}> {{"
    ));
    w.indent();

    let optional_count = vl.fields.iter().filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_))).count();
    emit_optional_header_read(optional_count, w);

    let mut optional_counter = 0;
    for fl in &vl.fields {
        if let ResolvedTypeRef::Optional(inner) = &fl.source_ty {
            let byte_idx = optional_counter / 8;
            let bit_idx = optional_counter % 8;
            optional_counter += 1;

            w.line(&format!("let __present = (__header[{byte_idx}] & (1 << {bit_idx})) != 0;"));

            // Extracted code call
            emit_field_mapping_arm(schema, fl, inner, "__present", w);
        } else {
            emit_field_read(schema, fl, w);
        }
    }

    if !vl.missing.is_empty() {
        w.blank();
        for mf in &vl.missing {
            let default = get_field_default_expr(mf.default.as_ref(), &mf.ty, schema);
            w.line(&format!("let {} = {default};", mf.target_name));
        }
    }

    w.blank();

    let latest = schema.versions.last().unwrap();
    w.line(&format!("Ok({name} {{"));
    w.indent();
    for field in &latest.fields {
        w.line(&format!("{},", field.name));
    }
    w.dedent();
    w.line("})");
    w.dedent();
    w.line("}");
}

fn emit_field_read(schema: &ResolvedSchema, fl: &FieldLineage, w: &mut CodeWriter) {
    match &fl.mapping {
        FieldMapping::Discard => {
            w.line(&emit_skip_stmt(&fl.source_ty));
        }
        FieldMapping::PassThrough { target_name } => {
            let expr = emit_read_expr(&fl.source_ty);
            w.line(&format!("let {target_name} = {expr};"));
        }
        FieldMapping::Cast {
            target_name,
            from,
            to,
        } => {
            let rhs = match emit_cast_value(schema, from, to) {
                CastExpr::Inline(e) | CastExpr::Block(e) => e,
            };
            w.line(&format!("let {target_name} = {rhs};"));
        },
    }
}

fn emit_read_expr(ty: &ResolvedTypeRef) -> String {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(_) => format!("{}(buf, pos)?", info.read_fn),
        ResolvedTypeRef::Enum(id) => {
            format!("{{ let __raw = read_varint64(buf, pos)? as u32; {}::try_from(__raw).map_err(|_| Error::InvalidEnumVariant)? }}", id.name)
        }
        ResolvedTypeRef::Bitset(id, _) => {
            format!("read_{}(buf, pos)?", id.name.to_snake_case())
        }
        ResolvedTypeRef::FixedString(_) => format!("{}(buf, pos)?", info.read_fn),
        ResolvedTypeRef::Array(inner) => {
            let inner_expr = emit_read_expr(inner);
            format!("{{ let __n = read_array_len(buf, pos)?; let mut __v = PojocVec::with_capacity(__n); for _ in 0..__n {{ __v.push({inner_expr}); }} __v }}")
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            let inner_expr = emit_read_expr(inner);
            format!("{{ let mut __arr: [_; {n}] = std::array::from_fn(|_| Default::default()); for __i in 0..{n} {{ __arr[__i] = {inner_expr}; }} __arr }}")
        }
        ResolvedTypeRef::DeltaArray(inner) => {
            let inner_rust = type_info(inner).rust_type;
            format!("read_delta_array::<{inner_rust}>(buf, pos)?")
        }
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            let inner_rust = type_info(inner).rust_type;
            format!("read_fixed_delta_array::<{inner_rust}, {n}>(buf, pos)?")
        }
        ResolvedTypeRef::Map(k_ty, v_ty) => {
            let ke = emit_read_expr(k_ty);
            let ve = emit_read_expr(v_ty);
            format!("{{ let __n = read_array_len(buf, pos)?; let mut __m = PojocMap::with_capacity(__n); for _ in 0..__n {{ let __k = {ke}; let __v = {ve}; __m.insert(__k, __v); }} __m }}")
        }
        ResolvedTypeRef::FixedMap(k_ty, v_ty, n) => {
            let ke = emit_read_expr(k_ty);
            let ve = emit_read_expr(v_ty);
            format!(
                "{{ let mut __m = PojocFixedMap::with_capacity({n}); \
         for _ in 0..{n} {{ let __k = {ke}; let __v = {ve}; __m.push((__k, __v)); }} \
         __m }}"
            )
        }
        ResolvedTypeRef::Tuple(elements) => {
            let inner = elements
                .iter()
                .map(emit_read_expr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
        ResolvedTypeRef::VFloat { min, step, backing, .. } => {
            format!(
                "(({}(buf, pos)? as f64) * {}f64 + {}f64) as f32",
                backing.read_fn(),
                step,
                min
            )
        }
        ResolvedTypeRef::Optional(inner) => {
            let inner_expr = emit_read_expr(inner);
            format!("{{ if read_u8(buf, pos)? != 0 {{ Some({inner_expr}) }} else {{ None }} }}")
        }
    }
}

fn emit_optional_header_read(optional_count: usize, w: &mut CodeWriter) {
    let header_bytes = (optional_count + 7) / 8;
    if header_bytes > 0 {
        w.line(&format!(
            "let __header = read_fixed_bytes::<{header_bytes}>(buf, pos)?;"
        ));
    }
}

fn emit_field_mapping_arm(
    schema: &ResolvedSchema,
    fl: &FieldLineage,
    inner: &ResolvedTypeRef,
    __present: &str,
    w: &mut CodeWriter,
) {
    match &fl.mapping {
        FieldMapping::Discard => {
            w.line(&format!("if {__present} {{"));
            w.indent();
            w.line(&emit_skip_stmt(inner));
            w.dedent();
            w.line("}");
        }
        FieldMapping::PassThrough { target_name } => {
            let inner_expr = emit_read_expr(inner);
            w.line(&format!(
                "let {target_name} = if {__present} {{ Some({inner_expr}) }} else {{ None }};"
            ));
        }
        FieldMapping::Cast { target_name, from: _, to } => {
            let (target_inner, optional_out) = match to {
                ResolvedTypeRef::Optional(t) => (&**t, true),
                t => (t, false),
            };
            let rhs = match emit_cast_value(schema, inner, target_inner) {
                CastExpr::Inline(e) | CastExpr::Block(e) => e,
            };
            if optional_out {
                w.line(&format!("let {target_name} = if {__present} {{ Some({rhs}) }} else {{ None }};"));
            } else {
                let default = type_info(to).default_expr;
                w.line(&format!("let {target_name} = if {__present} {{ {rhs} }} else {{ {default} }};"));
            }
        }
    }
}

fn get_field_default_expr(
    default_val: Option<&DefaultValue>,
    type_ref: &ResolvedTypeRef,
    schema: &ResolvedSchema,
) -> String {
    if let ResolvedTypeRef::FixedMap(_, _, n) = type_ref {
        return format!(
            "{{ let mut __m = PojocFixedMap::with_capacity({n}); \
             for _ in 0..{n} {{ __m.push((Default::default(), Default::default())); }} \
             __m }}"
        );
    }
    
    match default_val {
        Some(DefaultValue::None) if matches!(type_ref, ResolvedTypeRef::Optional(_)) => {
            "None".to_string()
        }

        Some(DefaultValue::None) if !matches!(type_ref, ResolvedTypeRef::Optional(_)) => {
            type_info(type_ref).default_expr
        }

        Some(DefaultValue::BitsetLiteral { ty_name, kvs })
        if matches!(type_ref, ResolvedTypeRef::Scalar(t) if is_primitive(&t.name)) =>
            {
                let bits = compute_bitset_literal_value(ty_name, kvs, schema);
                let rust_type = type_info(type_ref).rust_type;
                format!("{bits}{rust_type}")
            }

        Some(DefaultValue::Array(els)) => match type_ref {
            ResolvedTypeRef::FixedArray(_, n)
            | ResolvedTypeRef::FixedDeltaArray(_, n) => {
                let mut parts: Vec<String> =
                    els.iter().map(|e| emit_default(e, schema)).collect();
                parts.truncate(*n);
                while parts.len() < *n {
                    parts.push("Default::default()".to_string());
                }
                format!("[{}]", parts.join(", "))
            }
            _ if els.is_empty() => type_info(type_ref).default_expr,
            _ => emit_default(default_val.unwrap(), schema),
        },

        Some(DefaultValue::Map(pairs)) if pairs.is_empty() => type_info(type_ref).default_expr,
        Some(other) => emit_default(other, schema),
        None => type_info(type_ref).default_expr,
    }
}

fn compute_bitset_literal_value(ty_name: &str, kvs: &[(String, bool)], schema: &ResolvedSchema) -> u64 {
    let bs = schema.bitsets.bitsets.iter()
        .filter(|(id, _)| id.name == *ty_name)
        .max_by_key(|(id, _)| id.version)
        .map(|(_, bs)| bs);

    let mut value: u64 = 0;
    if let Some(bs) = bs {
        for (flag_name, set) in kvs {
            if *set {
                if let Some(idx) = bs.variants.iter().position(|v| v == flag_name) {
                    value |= 1u64 << idx;
                }
            }
        }
    }
    value
}

enum CastExpr {
    Inline(String),
    Block(String),
}

fn emit_cast_value(schema: &ResolvedSchema, from: &ResolvedTypeRef, to: &ResolvedTypeRef) -> CastExpr {
    use ResolvedTypeRef::*;
    match (from, to) {
        (Scalar(f), Scalar(t)) if is_primitive(&f.name) && is_primitive(&t.name) => {
            let fi = type_info(from);
            let ti = type_info(to);
            if f.name == t.name {
                CastExpr::Inline(format!("{}(buf, pos)?", fi.read_fn))
            } else {
                CastExpr::Inline(format!("{}(buf, pos)? as {}", fi.read_fn, ti.rust_type))
            }
        }

        (Scalar(f), Scalar(t)) => CastExpr::Block(struct_cast_block(schema, f, t)),

        (FixedString(from_n), FixedString(to_n)) => {
            let copy_n = (*from_n).min(*to_n);
            CastExpr::Block(format!(
                "{{ let __src = read_fixed_bytes::<{from_n}>(buf, pos)?; \
                 let mut __dst = [0u8; {to_n}]; \
                 __dst[..{copy_n}].copy_from_slice(&__src[..{copy_n}]); \
                 __dst }}"
            ))
        }

        (FixedMap(fk, fv, from_n), FixedMap(_, _, to_n)) => {
            let ke = emit_read_expr(fk);
            let ve = emit_read_expr(fv);
            CastExpr::Block(format!(
                "{{ let mut __m = PojocFixedMap::with_capacity({to_n}); \
                 for _ in 0..{from_n} {{ let __k = {ke}; let __v = {ve}; __m.push((__k, __v)); }} \
                 __m }}"
            ))
        }

        (FixedArray(elem, n), Map(_, v_ty)) => {
            let elem_expr = emit_read_expr(elem);
            let v_rust = type_info(v_ty).rust_type;
            CastExpr::Block(format!(
                "{{ let mut __m = PojocMap::with_capacity({n}); \
                 for __i in 0..{n} {{ let __v: {v_rust} = {elem_expr}; __m.insert(__i as i32, __v); }} \
                 __m }}"
            ))
        }

        (Bitset(id, width), Scalar(t)) if is_primitive(&t.name) => {
            let to_rust = type_info(to).rust_type;
            let read_fn = format!("read_{}", id.name.to_snake_case());
            let expr = match width {
                1 => format!("{read_fn}(buf, pos)?.0[0] as {to_rust}"),
                2 => format!("u16::from_le_bytes({read_fn}(buf, pos)?.0) as {to_rust}"),
                _ => format!("u32::from_le_bytes({read_fn}(buf, pos)?.0) as {to_rust}"),
            };
            CastExpr::Inline(expr)
        }

        (Scalar(f), Optional(to_inner)) if is_primitive(&f.name) => {
            match &**to_inner {
                Scalar(t) if is_primitive(&t.name) => {
                    let fi = type_info(from);
                    if f.name == t.name {
                        CastExpr::Inline(format!("Some({}(buf, pos)?)", fi.read_fn))
                    } else {
                        let ti = type_info(to_inner);
                        CastExpr::Inline(format!("Some({}(buf, pos)? as {})", fi.read_fn, ti.rust_type))
                    }
                }
                _ => CastExpr::Inline(format!("Some({})", emit_read_expr(from))),
            }
        }

        (FixedDeltaArray(from_elem, from_n), FixedDeltaArray(_, to_n)) => {
            let elem_rust = type_info(from_elem).rust_type;
            let elem_default = type_info(from_elem).default_expr;
            let copy_n = (*from_n).min(*to_n);
            CastExpr::Block(format!(
                "{{ let __src = read_fixed_delta_array::<{elem_rust}, {from_n}>(buf, pos)?; \
                 let mut __dst: [{elem_rust}; {to_n}] = std::array::from_fn(|_| {elem_default}); \
                 for __i in 0..{copy_n} {{ __dst[__i] = __src[__i]; }} \
                 __dst }}"
            ))
        }

        _ => CastExpr::Inline(emit_read_expr(from)),
    }
}

fn struct_cast_block(schema: &ResolvedSchema, from: &TypeId, to: &TypeId) -> String {
    let mut sub = CodeWriter::default();
    sub.line("{");
    sub.indent();
    emit_struct_cast_body(schema, from, to, &mut sub);
    sub.dedent();
    sub.line("}");
    sub.finish()
}

fn emit_struct_cast_body(
    schema: &ResolvedSchema,
    from: &TypeId,
    to: &TypeId,
    w: &mut CodeWriter,
) {
    let from_type = schema.types.types.get(from).expect("struct cast source not found");
    let to_type = schema.types.types.get(to).expect("struct cast target not found");

    let to_by_id: HashMap<FieldId, &FieldIR> = to_type.fields.iter().map(|f| (f.id, f)).collect();
    let from_ids: HashSet<FieldId> = from_type.fields.iter().map(|f| f.id).collect();
    let to_ids: HashSet<FieldId> = to_type.fields.iter().map(|f| f.id).collect();

    for src in &from_type.fields {
        let expr = emit_read_expr(&src.ty);
        if to_ids.contains(&src.id) {
            let dst_name = &to_by_id[&src.id].name;
            w.line(&format!("let {dst_name} = {expr};"));
        } else {
            w.line(&format!("let _ = {expr};"));
        }
    }

    for dst in &to_type.fields {
        if !from_ids.contains(&dst.id) {
            let default = get_field_default_expr(dst.default.as_ref(), &dst.ty, schema);
            w.line(&format!("let {} = {default};", dst.name));
        }
    }

    w.line(&format!("{} {{", to.name));
    w.indent();
    for f in &to_type.fields {
        w.line(&format!("{},", f.name));
    }
    w.dedent();
    w.line("}");
}