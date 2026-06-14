use heck::ToSnakeCase;
use super::writer::CodeWriter;
use pojoc_core::types::*;
use pojoc_schema::ir::lineage::FieldMapping;
use pojoc_schema::ir::types::*;
use crate::get_latest_versions;

pub fn emit_encode_function(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let latest = schema.versions.last().unwrap();

    emit_bitset_writers(schema, w);
    emit_type_writers(schema, w);

    w.line(&format!(
        "pub fn encode_payload(buf: &mut Vec<u8>, value: &{name}) {{"
    ));
    w.indent();

    // Map struct-declaration fields by name to re-index them into the lineage order
    let fields_by_name: std::collections::HashMap<String, &FieldIR> =
        latest.fields.iter().map(|f| (f.name.clone(), f)).collect();

    let latest_vl = schema.lineage.versions.last().unwrap();
    let mut ordered_fields = Vec::new();
    for fl in &latest_vl.fields {
        let target_name = match &fl.mapping {
            FieldMapping::PassThrough { target_name } => target_name,
            FieldMapping::Cast { target_name, .. } => target_name,
            FieldMapping::Discard => continue,
        };
        if let Some(f) = fields_by_name.get(target_name) {
            ordered_fields.push((*f).clone());
        }
    }

    emit_optional_header_write(&ordered_fields, w);

    emit_fields_write_loop(&ordered_fields, w);

    w.dedent();
    w.line("}");
}

fn emit_type_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.types.types, |id| id.name.clone(), |id| id.version);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let buf_param = if resolved.fields.is_empty() { "_buf" } else { "buf" };
        let value_param = if resolved.fields.is_empty() { "_value" } else { "value" };
        let fn_name = format!("write_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}({buf_param}: &mut Vec<u8>, {value_param}: &{name}) {{"
        ));
        w.indent();

        emit_optional_header_write(&resolved.fields, w);

        emit_fields_write_loop(&resolved.fields, w);

        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_bitset_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.bitsets.bitsets, |id| id.name.clone(), |id| id.version);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let fn_name = format!("write_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn {fn_name}(buf: &mut Vec<u8>, value: &{name}) {{"
        ));
        w.indent();
        w.line("write_fixed_bytes(buf, &value.0);");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_write_expr(ty: &ResolvedTypeRef, accessor: &str, w: &mut CodeWriter) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) => {
            if normalize_type(&id.name) == "string" {
                w.line(&format!("{}(buf, &{accessor});", info.write_fn));
            } else {
                w.line(&format!("{}(buf, {accessor});", info.write_fn));
            }
        }
        ResolvedTypeRef::Enum(_) => {
            w.line(&format!("write_varint64(buf, {accessor} as u64);"));
        }
        ResolvedTypeRef::Bitset(id, _) => {
            w.line(&format!(
                "write_{}(buf, &{accessor});",
                id.name.to_snake_case()
            ));
        }
        ResolvedTypeRef::Scalar(_) => {
            w.line(&format!("{}(buf, &{accessor});", info.write_fn));
        }
        ResolvedTypeRef::Array(inner) => {
            w.line(&format!("write_array_len(buf, {accessor}.len());"));
            w.line(&format!("for item in {accessor}.iter() {{"));
            w.indent();
            let item_accessor = match inner.as_ref() {
                ResolvedTypeRef::Scalar(id)
                if is_primitive(&id.name) && normalize_type(&id.name) != "string" => "*item",
                ResolvedTypeRef::VFloat { .. } => "*item",
                _ => "item",
            };
            emit_write_expr(inner, item_accessor, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::FixedArray(inner, _n) => {
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            let item_accessor = match inner.as_ref() {
                ResolvedTypeRef::Scalar(id)
                if is_primitive(&id.name) && normalize_type(&id.name) != "string" => "*__item",
                ResolvedTypeRef::VFloat { .. } => "*__item",
                _ => "__item",
            };
            emit_write_expr(inner, item_accessor, w);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::DeltaArray(_) => {
            w.line(&format!("write_delta_array(buf, {accessor}.as_slice());"));
        }
        ResolvedTypeRef::FixedDeltaArray(_, _) => {
            w.line(&format!("write_fixed_delta_array(buf, &{accessor}[..]);"));
        }
        ResolvedTypeRef::FixedString(_n) => {
            w.line(&format!("{}(buf, &{accessor});", info.write_fn));
        }
        ResolvedTypeRef::Map(k_ty, v_ty) => {
            w.line(&format!("write_array_len(buf, {accessor}.len());"));
            w.line(&format!("for (__k, __v) in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(k_ty, &map_entry_accessor(k_ty, "__k"), w);
            emit_write_expr(v_ty, &map_entry_accessor(v_ty, "__v"), w);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::FixedMap(k_ty, v_ty, n) => {
            w.line(&format!("assert_eq!({accessor}.inner.len(), {n}, \"FixedMap length mismatch\");"));
            w.line(&format!("for (__k, __v) in {accessor}.inner.iter() {{"));
            w.indent();
            emit_write_expr(k_ty, &map_entry_accessor(k_ty, "__k"), w);
            emit_write_expr(v_ty, &map_entry_accessor(v_ty, "__v"), w);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::Tuple(elements) => {
            for (i, elem) in elements.iter().enumerate() {
                emit_write_expr(elem, &format!("{accessor}.{i}"), w);
            }
        }
        ResolvedTypeRef::VFloat { min, step, backing, .. } => {
            w.line(&format!(
                "{}(buf, (({accessor} as f64 - {}f64) / {}f64).round() as {});",
                info.write_fn, min, step, backing.rust_int_type()
            ));
        }
        ResolvedTypeRef::Optional(inner) => {
            w.line(&format!("match &{accessor} {{"));
            w.indent();
            w.line("Some(__val) => {");
            w.indent();
            w.line("write_u8(buf, 1);");
            let inner_accessor = get_optional_inner_accessor(inner);
            emit_write_expr(inner, inner_accessor, w);
            w.dedent();
            w.line("}");
            w.line("None => write_u8(buf, 0),");
            w.dedent();
            w.line("}");
        }
    }
}

pub fn emit_size_hint(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let latest = schema.versions.last().unwrap();

    emit_type_size_hints(schema, w);

    w.line(&format!("pub fn size_hint(value: &{name}) -> usize {{"));
    w.indent();
    w.line("let mut size = 5usize; // envelope: 1 version varint + 4 byte length");

    // Align structural sizing estimation loops with lineage tracking array
    let fields_by_name: std::collections::HashMap<String, &FieldIR> =
        latest.fields.iter().map(|f| (f.name.clone(), f)).collect();

    let latest_vl = schema.lineage.versions.last().unwrap();
    let mut ordered_fields = Vec::new();
    for fl in &latest_vl.fields {
        let target_name = match &fl.mapping {
            FieldMapping::PassThrough { target_name } => target_name,
            FieldMapping::Cast { target_name, .. } => target_name,
            FieldMapping::Discard => continue,
        };
        if let Some(f) = fields_by_name.get(target_name) {
            ordered_fields.push((*f).clone());
        }
    }

    emit_optional_header_size(&ordered_fields, w);

    emit_fields_size_loop(&ordered_fields, w, schema);

    w.line("size");
    w.dedent();
    w.line("}");
    w.blank();
}

fn emit_type_size_hints(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.types.types, |id| id.name.clone(), |id| id.version);

    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();

    for name in names {
        let (_, resolved) = latest[name];
        let fn_name = format!("size_hint_{}", name.to_snake_case());
        w.line("#[allow(dead_code)]");
        w.line("#[allow(unused_variables)]");
        w.line(&format!("fn {fn_name}(value: &{name}) -> usize {{"));
        w.indent();
        if resolved.fields.is_empty() {
            w.line("let size = 0usize;");
        } else {
            w.line("let mut size = 0usize;");
        }

        emit_optional_header_size(&resolved.fields, w);

        emit_fields_size_loop(&resolved.fields, w, schema);

        w.line("size");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_size_expr(
    ty: &ResolvedTypeRef,
    accessor: &str,
    w: &mut CodeWriter,
    schema: &ResolvedSchema,
) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(_) => {
            w.line(&format!("size += {};", info.size_expr(accessor)));
        }
        ResolvedTypeRef::Enum(_) => {
            w.line(&format!("size += varint_size({accessor} as usize);"));
        }
        ResolvedTypeRef::Bitset(id, _) => {
            if let Some(bs) = schema.bitsets.bitsets.get(id) {
                let computed_len = (bs.variants.len() + 7) / 8;
                w.line(&format!("size += {computed_len};"));
            } else {
                w.line("size += 1;");
            }
        }
        ResolvedTypeRef::Array(inner) => {
            let inner_info = type_info(inner);
            if let WireSize::Fixed(elem_size) = inner_info.wire_size {
                w.line(&format!("size += varint_size({accessor}.len());"));
                w.line(&format!("size += {accessor}.len() * {elem_size};"));
                return;
            }
            w.line(&format!("size += varint_size({accessor}.len());"));
            w.line(&format!("for item in {accessor}.iter() {{"));
            w.indent();
            emit_size_expr(inner, "item", w, schema);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::FixedArray(inner, n) => {
            let inner_info = type_info(inner);
            if let WireSize::Fixed(elem_size) = inner_info.wire_size {
                w.line(&format!("size += {n} * {elem_size};"));
                return;
            }
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            emit_size_expr(inner, "__item", w, schema);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::DeltaArray(_) => {
            w.line(&format!("size += delta_array_size_hint({accessor}.as_slice());"));
        }
        ResolvedTypeRef::FixedDeltaArray(_, _) => {
            w.line(&format!("size += fixed_delta_array_size_hint(&{accessor}[..]);"));
        }
        ResolvedTypeRef::FixedString(n) => {
            w.line(&format!("size += {n};"));
        }
        ResolvedTypeRef::Map(k_ty, v_ty) => {
            w.line(&format!("size += varint_size({accessor}.len());"));
            w.line(&format!("for (__k, __v) in {accessor}.iter() {{"));
            w.indent();
            emit_size_expr(k_ty, "__k", w, schema);
            emit_size_expr(v_ty, "__v", w, schema);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::FixedMap(k_ty, v_ty, n) => {
            let k_info = type_info(k_ty);
            let v_info = type_info(v_ty);
            if let (WireSize::Fixed(ks), WireSize::Fixed(vs)) = (&k_info.wire_size, &v_info.wire_size) {
                w.line(&format!("size += {n} * ({ks} + {vs});"));
                return;
            }
            let k_rust = k_info.rust_type;
            let v_rust = v_info.rust_type;
            w.line(&format!("for __i in 0..{n} {{"));
            w.indent();
            w.line(&format!("let __default: ({k_rust}, {v_rust}) = Default::default();"));
            w.line(&format!("let (__k, __v) = {accessor}.inner.get(__i).unwrap_or(&__default);"));
            emit_size_expr(k_ty, "__k", w, schema);
            emit_size_expr(v_ty, "__v", w, schema);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::Tuple(elements) => {
            for (i, elem) in elements.iter().enumerate() {
                emit_size_expr(elem, &format!("{accessor}.{i}"), w, schema);
            }
        }
        ResolvedTypeRef::VFloat { .. } => {
            w.line(&format!("size += {};", info.size_expr(accessor)));
        }
        ResolvedTypeRef::Optional(inner) => {
            w.line("size += 1;");
            w.line(&format!("if let Some(__val) = &{accessor} {{"));
            w.indent();
            emit_size_expr(inner, "__val", w, schema);
            w.dedent();
            w.line("}");
        }
    }
}

fn map_entry_accessor(ty: &ResolvedTypeRef, var: &str) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id)
        if is_primitive(&id.name) && normalize_type(&id.name) != "string" => format!("*{var}"),
        ResolvedTypeRef::VFloat { .. } => format!("*{var}"),
        _ => var.to_string(),
    }
}

fn emit_optional_header_write(fields: &[FieldIR], w: &mut CodeWriter) {
    let optional_fields: Vec<&FieldIR> = fields
        .iter()
        .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
        .collect();
    let header_bytes = (optional_fields.len() + 7) / 8;

    if header_bytes > 0 {
        w.line(&format!("let mut __header = [0u8; {header_bytes}];"));
        for (idx, field) in optional_fields.iter().enumerate() {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            w.line(&format!(
                "if value.{}.is_some() {{ __header[{byte_idx}] |= 1 << {bit_idx}; }}",
                field.name
            ));
        }
        w.line("write_fixed_bytes(buf, &__header);");
    }
}

fn emit_optional_header_size(fields: &[FieldIR], w: &mut CodeWriter) {
    let optional_count = fields
        .iter()
        .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
        .count();
    let header_bytes = (optional_count + 7) / 8;
    if header_bytes > 0 {
        w.line(&format!("size += {header_bytes}; // bitpacked optionals header"));
    }
}

fn emit_fields_write_loop(fields: &[FieldIR], w: &mut CodeWriter) {
    for field in fields {
        let accessor = format!("value.{}", field.name);
        match &field.ty {
            ResolvedTypeRef::Optional(inner) => {
                w.line(&format!("if let Some(__val) = &{accessor} {{"));
                w.indent();
                let inner_accessor = get_optional_inner_accessor(inner);
                emit_write_expr(inner, inner_accessor, w);
                w.dedent();
                w.line("}");
            }
            _ => {
                emit_write_expr(&field.ty, &accessor, w);
            }
        }
    }
}

fn emit_fields_size_loop(fields: &[FieldIR], w: &mut CodeWriter, schema: &ResolvedSchema) {
    for field in fields {
        let accessor = format!("value.{}", field.name);
        match &field.ty {
            ResolvedTypeRef::Optional(inner) => {
                w.line(&format!("if let Some(__val) = &{accessor} {{"));
                w.indent();
                emit_size_expr(inner, "__val", w, schema);
                w.dedent();
                w.line("}");
            }
            _ => {
                emit_size_expr(&field.ty, &accessor, w, schema);
            }
        }
    }
}

fn get_optional_inner_accessor(inner: &ResolvedTypeRef) -> &'static str {
    match inner {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) && normalize_type(&id.name) != "string" => "*__val",
        ResolvedTypeRef::VFloat { .. } => "*__val",
        _ => "__val",
    }
}