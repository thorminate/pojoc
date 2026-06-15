// encode.rs
use heck::ToSnakeCase;
use super::writer::CodeWriter;
use pojoc_core::types::*;
use pojoc_schema::ir::lineage::*;
use pojoc_schema::ir::types::*;
use crate::get_latest_versions;

pub fn emit_encode_helpers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    emit_bitset_writers(schema, w);
    emit_type_writers(schema, w);
}

pub fn emit_encode_vn_functions(schema: &ResolvedSchema, w: &mut CodeWriter) {
    for vl in &schema.lineage.versions {
        emit_encode_vn_fn(schema, vl, w);
        w.blank();
    }
}

pub fn emit_size_hint(schema: &ResolvedSchema, w: &mut CodeWriter) {
    emit_type_size_hints(schema, w);

    let name = &schema.name_hint;
    let ordered = lineage_ordered_fields(schema);

    w.line(&format!("pub fn size_hint(value: &{name}) -> usize {{"));
    w.indent();
    w.line("let mut size = 5usize; // envelope: 1 version varint + 4 byte length");
    emit_optional_header_size(&ordered, w);
    emit_fields_size_loop(&ordered, w, schema);
    w.line("size");
    w.dedent();
    w.line("}");
    w.blank();
}

fn lineage_ordered_fields(schema: &ResolvedSchema) -> Vec<FieldIR> {
    let latest = schema.versions.last().unwrap();
    let by_name: std::collections::HashMap<&str, &FieldIR> =
        latest.fields.iter().map(|f| (f.name.as_str(), f)).collect();

    schema.lineage.versions.last().unwrap().fields.iter()
        .filter_map(|fl| {
            let name = match &fl.mapping {
                FieldMapping::PassThrough { target_name } => target_name.as_str(),
                FieldMapping::Cast { target_name, .. } => target_name.as_str(),
                FieldMapping::Discard => return None,
            };
            by_name.get(name).map(|f| (*f).clone())
        })
        .collect()
}

fn deref_if_copy(ty: &ResolvedTypeRef, var: &str) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id)
        if is_primitive(&id.name) && normalize_type(&id.name) != "string" =>
            {
                format!("*{var}")
            }
        ResolvedTypeRef::VFloat { .. } => format!("*{var}"),
        _ => var.to_string(),
    }
}

fn emit_bitset_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.bitsets.bitsets, |id| id.name.clone(), |id| id.version);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();
    for name in names {
        w.line("#[allow(dead_code)]");
        w.line(&format!(
            "fn write_{}(buf: &mut Vec<u8>, value: &{name}) {{",
            name.to_snake_case()
        ));
        w.indent();
        w.line("write_fixed_bytes(buf, &value.0);");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_type_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.types.types, |id| id.name.clone(), |id| id.version);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();
    for name in names {
        let (_, resolved) = latest[name];
        let (buf, val) = if resolved.fields.is_empty() { ("_buf", "_value") } else { ("buf", "value") };
        w.line("#[allow(dead_code)]");
        w.line(&format!("fn write_{}({buf}: &mut Vec<u8>, {val}: &{name}) {{", name.to_snake_case()));
        w.indent();
        emit_optional_header_write(&resolved.fields, w);
        emit_fields_write_loop(&resolved.fields, w);
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_optional_header_write(fields: &[FieldIR], w: &mut CodeWriter) {
    let opt: Vec<&FieldIR> = fields.iter()
        .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
        .collect();
    let header_bytes = (opt.len() + 7) / 8;
    if header_bytes == 0 { return; }
    w.line(&format!("let mut __header = [0u8; {header_bytes}];"));
    for (idx, f) in opt.iter().enumerate() {
        w.line(&format!(
            "if value.{}.is_some() {{ __header[{}] |= 1 << {}; }}",
            f.name, idx / 8, idx % 8
        ));
    }
    w.line("write_fixed_bytes(buf, &__header);");
}

fn emit_fields_write_loop(fields: &[FieldIR], w: &mut CodeWriter) {
    for field in fields {
        let accessor = format!("value.{}", field.name);
        match &field.ty {
            ResolvedTypeRef::Optional(inner) => {
                w.line(&format!("if let Some(__val) = &{accessor} {{"));
                w.indent();
                emit_write_expr(inner, &deref_if_copy(inner, "__val"), None, w);
                w.dedent();
                w.line("}");
            }
            _ => emit_write_expr(&field.ty, &accessor, None, w),
        }
    }
}

fn emit_write_expr(
    ty: &ResolvedTypeRef,
    accessor: &str,
    vn: Option<&ResolvedSchema>,
    w: &mut CodeWriter,
) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) => {
            if normalize_type(&id.name) == "string" {
                w.line(&format!("{}(buf, &{accessor});", info.write_fn));
            } else {
                w.line(&format!("{}(buf, {accessor});", info.write_fn));
            }
        }

        ResolvedTypeRef::Enum(id) => {
            if let Some(schema) = vn {
                let max = schema.enums.enums.get(id)
                    .and_then(|e| e.variants.iter().map(|v| v.wire_value).max())
                    .unwrap_or(0);
                w.line(&format!(
                    "{{ let __disc = {accessor} as u32; \
                     write_varint64(buf, if __disc <= {max} {{ __disc as u64 }} else {{ 0u64 }}); }}"
                ));
            } else {
                w.line(&format!("write_varint64(buf, {accessor} as u64);"));
            }
        }

        ResolvedTypeRef::Bitset(id, _) => {
            w.line(&format!("write_{}(buf, &{accessor});", id.name.to_snake_case()));
        }

        ResolvedTypeRef::Scalar(_) => {
            // Non-primitive struct type — delegate to its write_X function.
            w.line(&format!("{}(buf, &{accessor});", info.write_fn));
        }

        ResolvedTypeRef::Array(inner) => {
            w.line(&format!("write_array_len(buf, {accessor}.len());"));
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(inner, &deref_if_copy(inner, "__item"), vn, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::FixedArray(inner, _) => {
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(inner, &deref_if_copy(inner, "__item"), vn, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::DeltaArray(_) => {
            w.line(&format!("write_delta_array(buf, {accessor}.as_slice());"));
        }

        ResolvedTypeRef::FixedDeltaArray(_, _) => {
            w.line(&format!("write_fixed_delta_array(buf, &{accessor}[..]);"));
        }

        ResolvedTypeRef::FixedString(_) => {
            w.line(&format!("{}(buf, &{accessor});", info.write_fn));
        }

        ResolvedTypeRef::Map(k_ty, v_ty) => {
            w.line(&format!("write_array_len(buf, {accessor}.len());"));
            w.line(&format!("for (__k, __v) in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(k_ty, &deref_if_copy(k_ty, "__k"), vn, w);
            emit_write_expr(v_ty, &deref_if_copy(v_ty, "__v"), vn, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::FixedMap(k_ty, v_ty, n) => {
            w.line(&format!(
                "assert_eq!({accessor}.inner.len(), {n}, \"FixedMap length mismatch\");"
            ));
            w.line(&format!("for (__k, __v) in {accessor}.inner.iter() {{"));
            w.indent();
            emit_write_expr(k_ty, &deref_if_copy(k_ty, "__k"), vn, w);
            emit_write_expr(v_ty, &deref_if_copy(v_ty, "__v"), vn, w);
            w.dedent();
            w.line("}");
        }

        ResolvedTypeRef::Tuple(elements) => {
            for (i, elem) in elements.iter().enumerate() {
                emit_write_expr(elem, &format!("{accessor}.{i}"), vn, w);
            }
        }

        ResolvedTypeRef::VFloat { min, step, backing, .. } => {
            w.line(&format!(
                "{}(buf, (({accessor} as f64 - {min}f64) / {step}f64).round() as {});",
                info.write_fn,
                backing.rust_int_type()
            ));
        }

        ResolvedTypeRef::Optional(inner) => {
            w.line(&format!("match &{accessor} {{"));
            w.indent();
            w.line("Some(__val) => {");
            w.indent();
            w.line("write_u8(buf, 1);");
            emit_write_expr(inner, &deref_if_copy(inner, "__val"), vn, w);
            w.dedent();
            w.line("}");
            w.line("None => write_u8(buf, 0),");
            w.dedent();
            w.line("}");
        }
    }
}

fn emit_encode_vn_fn(schema: &ResolvedSchema, vl: &VersionLineage, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let v = vl.version;

    w.line(&format!("pub fn encode_v{v}(buf: &mut Vec<u8>, value: &{name}) {{"));
    w.indent();

    let opt_fields: Vec<&FieldLineage> = vl.fields.iter()
        .filter(|fl| matches!(fl.source_ty, ResolvedTypeRef::Optional(_)))
        .collect();
    let header_bytes = (opt_fields.len() + 7) / 8;
    if header_bytes > 0 {
        w.line(&format!("let mut __header = [0u8; {header_bytes}];"));
        for (idx, fl) in opt_fields.iter().enumerate() {
            emit_vn_optional_header_bit(fl, idx / 8, idx % 8, w);
        }
        w.line("write_fixed_bytes(buf, &__header);");
    }

    for fl in &vl.fields {
        if let ResolvedTypeRef::Optional(inner_src) = &fl.source_ty {
            emit_vn_optional_body(schema, fl, inner_src, w);
        } else {
            emit_vn_nonoptional_field(schema, fl, w);
        }
    }

    w.dedent();
    w.line("}");
}

fn emit_vn_optional_header_bit(fl: &FieldLineage, byte_idx: usize, bit_idx: usize, w: &mut CodeWriter) {
    match &fl.mapping {
        FieldMapping::Discard => {}
        FieldMapping::PassThrough { target_name } => {
            w.line(&format!(
                "if value.{target_name}.is_some() {{ __header[{byte_idx}] |= 1 << {bit_idx}; }}"
            ));
        }
        FieldMapping::Cast { target_name, to, .. } => {
            if matches!(to, ResolvedTypeRef::Optional(_)) {
                w.line(&format!(
                    "if value.{target_name}.is_some() {{ __header[{byte_idx}] |= 1 << {bit_idx}; }}"
                ));
            } else {
                w.line(&format!(
                    "__header[{byte_idx}] |= 1 << {bit_idx}; // promoted to mandatory"
                ));
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
            w.line(&format!("if let Some(__val) = &value.{target_name} {{"));
            w.indent();
            emit_write_expr(inner_src, &deref_if_copy(inner_src, "__val"), Some(schema), w);
            w.dedent();
            w.line("}");
        }
        FieldMapping::Cast { target_name, from, to } => {
            match (from, to) {
                (ResolvedTypeRef::Optional(f_inner), ResolvedTypeRef::Optional(t_inner)) => {
                    w.line(&format!("if let Some(__val) = &value.{target_name} {{"));
                    w.indent();
                    emit_vn_cast_value(schema, f_inner, t_inner, &deref_if_copy(t_inner, "__val"), w);
                    w.dedent();
                    w.line("}");
                }
                (ResolvedTypeRef::Optional(f_inner), t_inner) => {
                    emit_vn_cast_value(schema, f_inner, t_inner, &format!("value.{target_name}"), w);
                }
                _ => {
                    w.line(&format!("if let Some(__val) = &value.{target_name} {{"));
                    w.indent();
                    emit_write_expr(inner_src, &deref_if_copy(inner_src, "__val"), Some(schema), w);
                    w.dedent();
                    w.line("}");
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
            emit_write_expr(
                &fl.source_ty,
                &format!("value.{target_name}"),
                Some(schema),
                w,
            );
        }
        FieldMapping::Cast { target_name, from, to } => {
            emit_vn_cast_value(schema, from, to, &format!("value.{target_name}"), w);
        }
    }
}

fn emit_vn_cast_value(
    schema: &ResolvedSchema,
    from: &ResolvedTypeRef,
    to: &ResolvedTypeRef,
    accessor: &str,
    w: &mut CodeWriter,
) {
    use ResolvedTypeRef::*;
    match (from, to) {

        (Scalar(f), Scalar(t)) if is_primitive(&f.name) && is_primitive(&t.name) => {
            let f_norm = normalize_type(&f.name);
            let t_norm = normalize_type(&t.name);
            if f_norm == t_norm {
                emit_write_expr(from, accessor, Some(schema), w);
            } else if f_norm == "string" || t_norm == "string" {
                emit_write_expr(from, accessor, Some(schema), w);
            } else {
                let f_info = type_info(from);
                w.line(&format!("{}(buf, {accessor} as {});", f_info.write_fn, f_info.rust_type));
            }
        }

        (Scalar(f_id), Scalar(_)) if !is_primitive(&f_id.name) => {
            w.line("{");
            w.indent();
            if let Some(historical_struct) = schema.types.types.get(f_id) {
                let value_binding = if historical_struct.fields.is_empty() { "_val" } else { "val" };
                w.line(&format!("let {value_binding} = &{accessor};"));

                let historical_opts: Vec<&FieldIR> = historical_struct.fields.iter()
                    .filter(|f| matches!(f.ty, Optional(_)))
                    .collect();
                let header_bytes = (historical_opts.len() + 7) / 8;
                if header_bytes > 0 {
                    w.line(&format!("let mut __nested_header = [0u8; {header_bytes}];"));
                    for (idx, f) in historical_opts.iter().enumerate() {
                        w.line(&format!(
                            "if {value_binding}.{}.is_some() {{ __nested_header[{}] |= 1 << {}; }}",
                            f.name, idx / 8, idx % 8
                        ));
                    }
                    w.line("write_fixed_bytes(buf, &__nested_header);");
                }

                let current_type_version = schema.types.types.iter()
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
                                emit_write_expr(inner, &deref_if_copy(inner, "__val"), Some(schema), w);
                                w.dedent();
                                w.line("}");
                            }
                            _ => emit_write_expr(&field.ty, &field_accessor, Some(schema), w),
                        }
                    } else {
                        if !matches!(field.ty, Optional(_)) {
                            emit_vn_default_write(&field.ty, schema, w);
                        }
                    }
                }
            } else {
                panic!("Historical type definition for {} missing in schema catalog.", f_id.name);
            }
            w.dedent();
            w.line("}");
        }

        (Enum(f_id), Enum(_)) => {
            let max = schema.enums.enums.get(f_id)
                .and_then(|e| e.variants.iter().map(|v| v.wire_value).max())
                .unwrap_or(0);
            w.line(&format!(
                "{{ let __disc = {accessor} as u32; \
                 write_varint64(buf, if __disc <= {max} {{ __disc as u64 }} else {{ 0u64 }}); }}"
            ));
        }

        (VFloat { min: f_min, step: f_step, backing: f_backing, .. }, VFloat { .. }) => {
            w.line(&format!(
                "{wfn}(buf, (({accessor} as f64 - {f_min}f64) / {f_step}f64).round() as {ri});",
                wfn = f_backing.write_fn(),
                ri = f_backing.rust_int_type(),
            ));
        }

        (FixedString(f_n), FixedString(t_n)) => {
            let copy_n = (*f_n).min(*t_n);
            w.line(&format!(
                "{{ let mut __tmp = [0u8; {f_n}]; \
                 __tmp[..{copy_n}].copy_from_slice(&{accessor}[..{copy_n}]); \
                 write_fixed_bytes::<{f_n}>(buf, &__tmp); }}"
            ));
        }

        (FixedArray(elem_ty, f_n), Map(_, _)) => {
            w.line("{");
            w.indent();
            w.line("let mut __written = 0usize;");
            w.line(&format!("for (_, __v) in {accessor}.iter().take({f_n}) {{"));
            w.indent();
            emit_write_expr(elem_ty, &deref_if_copy(elem_ty, "__v"), None, w);
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
                emit_write_expr(elem_ty, &deref_if_copy(elem_ty, "__item"), None, w);
                w.dedent();
                w.line("}");
            } else {
                w.line(&format!("for __item in {accessor}.iter() {{"));
                w.indent();
                emit_write_expr(elem_ty, &deref_if_copy(elem_ty, "__item"), None, w);
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
                w.line(&format!("write_fixed_delta_array(buf, &{accessor}[..{f_n}]);"));
            } else {
                let elem_rust = type_info(elem_ty).rust_type;
                let elem_default = type_info(elem_ty).default_expr;
                w.line("{");
                w.indent();
                w.line(&format!("let mut __tmp: [{elem_rust}; {f_n}] = [{elem_default}; {f_n}];"));
                w.line(&format!("__tmp[..{t_n}].copy_from_slice(&{accessor}[..]);"));
                w.line("write_fixed_delta_array(buf, &__tmp[..]);");
                w.dedent();
                w.line("}");
            }
        }

        (Array(f_elem), DeltaArray(_)) => {
            w.line(&format!("write_array_len(buf, {accessor}.len());"));
            w.line(&format!("for __item in {accessor}.iter() {{"));
            w.indent();
            emit_write_expr(f_elem, &deref_if_copy(f_elem, "__item"), None, w);
            w.dedent();
            w.line("}");
        }

        (DeltaArray(_), Array(_)) => {
            // vN used delta encoding.
            w.line(&format!("write_delta_array(buf, {accessor}.as_slice());"));
        }

        (FixedMap(fk, fv, f_n), FixedMap(_, _, _)) => {
            w.line(&format!("for __i in 0usize..{f_n} {{"));
            w.indent();
            w.line(&format!("if let Some((__k, __v)) = {accessor}.inner.get(__i) {{"));
            w.indent();
            emit_write_expr(fk, &deref_if_copy(fk, "__k"), None, w);
            emit_write_expr(fv, &deref_if_copy(fv, "__v"), None, w);
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

        (Bitset(_, width), Scalar(_)) => {
            match width {
                1 => w.line(&format!("write_u8(buf, {accessor} as u8);")),
                2 => w.line(&format!("write_u16(buf, {accessor} as u16);")),
                _ => w.line(&format!("write_u32(buf, {accessor} as u32);")),
            }
        }

        (Scalar(f), Bitset(_, width)) if is_primitive(&f.name) => {
            match width {
                1 => w.line(&format!("write_u8(buf, {accessor}.0[0]);")),
                2 => w.line(&format!(
                    "write_u16(buf, u16::from_le_bytes([{accessor}.0[0], {accessor}.0[1]]));"
                )),
                _ => w.line(&format!(
                    "write_u32(buf, u32::from_le_bytes(\
                     [{accessor}.0[0], {accessor}.0[1], {accessor}.0[2], {accessor}.0[3]]));"
                )),
            }
        }

        (from_ty, Optional(t_inner)) => {
            w.line(&format!("match &{accessor} {{"));
            w.indent();
            w.line("Some(__val) => {");
            w.indent();
            emit_vn_cast_value(schema, from_ty, t_inner, &deref_if_copy(t_inner, "__val"), w);
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

        _ => emit_write_expr(from, accessor, Some(schema), w),
    }
}

fn emit_vn_default_write(ty: &ResolvedTypeRef, schema: &ResolvedSchema, w: &mut CodeWriter) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) => {
            if normalize_type(&id.name) == "string" {
                w.line(&format!("{}(buf, &{});", info.write_fn, info.default_expr));
            } else {
                w.line(&format!("{}(buf, {});", info.write_fn, info.default_expr));
            }
        }
        ResolvedTypeRef::Scalar(id) => {
            w.line("{");
            w.indent();
            if let Some(historical_struct) = schema.types.types.get(id) {
                let historical_opts_count = historical_struct.fields.iter()
                    .filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_)))
                    .count();
                let header_bytes = (historical_opts_count + 7) / 8;
                if header_bytes > 0 {
                    w.line(&format!("write_fixed_bytes(buf, &[0u8; {header_bytes}]);"));
                }

                for field in &historical_struct.fields {
                    if !matches!(field.ty, ResolvedTypeRef::Optional(_)) {
                        emit_vn_default_write(&field.ty, schema, w);
                    }
                }
            } else {
                w.line(&format!("write_{}(buf, &{}::default());", id.name.to_snake_case(), id.name));
            }
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::Enum(_) => {
            w.line("write_varint64(buf, 0u64); // Unknown");
        }
        ResolvedTypeRef::Bitset(id, _) => {
            w.line(&format!(
                "write_{}(buf, &{}::default());",
                id.name.to_snake_case(), id.name
            ));
        }
        ResolvedTypeRef::Array(_) => {
            w.line("write_array_len(buf, 0); // empty");
        }
        ResolvedTypeRef::DeltaArray(_) => {
            w.line("write_varint32(buf, 0u32); // empty delta array");
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            w.line(&format!("for _ in 0..{n} {{"));
            w.indent();
            emit_vn_default_write(inner, schema, w);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            let inner_rust = type_info(inner).rust_type;
            let inner_default = type_info(inner).default_expr;
            w.line(&format!(
                "write_fixed_delta_array(buf, &[{inner_rust}::default(); {n}][..]);"
            ));
            let _ = inner_default;
        }
        ResolvedTypeRef::FixedString(n) => {
            w.line(&format!("write_fixed_bytes::<{n}>(buf, &[0u8; {n}]);"));
        }
        ResolvedTypeRef::Map(_, _) => {
            w.line("write_array_len(buf, 0); // empty map");
        }
        ResolvedTypeRef::FixedMap(k, v, n) => {
            w.line(&format!("for _ in 0..{n} {{"));
            w.indent();
            emit_vn_default_write(k, schema, w);
            emit_vn_default_write(v, schema, w);
            w.dedent();
            w.line("}");
        }
        ResolvedTypeRef::Tuple(elements) => {
            for elem in elements {
                emit_vn_default_write(elem, schema, w);
            }
        }
        ResolvedTypeRef::VFloat { backing, .. } => {
            w.line(&format!("{}(buf, 0{});", backing.write_fn(), backing.rust_int_type()));
        }
        ResolvedTypeRef::Optional(_) => {}
    }
}

fn emit_type_size_hints(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let latest = get_latest_versions(&schema.types.types, |id| id.name.clone(), |id| id.version);
    let mut names: Vec<&String> = latest.keys().collect();
    names.sort();
    for name in names {
        let (_, resolved) = latest[name];
        w.line("#[allow(dead_code)]");
        w.line("#[allow(unused_variables)]");
        w.line(&format!("fn size_hint_{}(value: &{name}) -> usize {{", name.to_snake_case()));
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

fn emit_optional_header_size(fields: &[FieldIR], w: &mut CodeWriter) {
    let n = fields.iter().filter(|f| matches!(f.ty, ResolvedTypeRef::Optional(_))).count();
    if n > 0 {
        w.line(&format!("size += {}; // bitpacked optionals header", (n + 7) / 8));
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
            _ => emit_size_expr(&field.ty, &accessor, w, schema),
        }
    }
}

fn emit_size_expr(ty: &ResolvedTypeRef, accessor: &str, w: &mut CodeWriter, schema: &ResolvedSchema) {
    let info = type_info(ty);
    match ty {
        ResolvedTypeRef::Scalar(_) => {
            w.line(&format!("size += {};", info.size_expr(accessor)));
        }
        ResolvedTypeRef::Enum(_) => {
            w.line(&format!("size += varint_size({accessor} as usize);"));
        }
        ResolvedTypeRef::Bitset(id, _) => {
            let computed_len = schema.bitsets.bitsets.get(id)
                .map(|bs| (bs.variants.len() + 7) / 8)
                .unwrap_or(1);
            w.line(&format!("size += {computed_len};"));
        }
        ResolvedTypeRef::Array(inner) => {
            let inner_info = type_info(inner);
            w.line(&format!("size += varint_size({accessor}.len());"));
            if let WireSize::Fixed(elem_size) = inner_info.wire_size {
                w.line(&format!("size += {accessor}.len() * {elem_size};"));
            } else {
                w.line(&format!("for item in {accessor}.iter() {{"));
                w.indent();
                emit_size_expr(inner, "item", w, schema);
                w.dedent();
                w.line("}");
            }
        }
        ResolvedTypeRef::FixedArray(inner, n) => {
            let inner_info = type_info(inner);
            if let WireSize::Fixed(elem_size) = inner_info.wire_size {
                w.line(&format!("size += {n} * {elem_size};"));
            } else {
                w.line(&format!("for __item in {accessor}.iter() {{"));
                w.indent();
                emit_size_expr(inner, "__item", w, schema);
                w.dedent();
                w.line("}");
            }
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
            } else {
                let k_rust = k_info.rust_type;
                let v_rust = v_info.rust_type;
                w.line(&format!("for __i in 0..{n} {{"));
                w.indent();
                w.line(&format!("let __default: ({k_rust}, {v_rust}) = Default::default();"));
                w.line(&format!(
                    "let (__k, __v) = {accessor}.inner.get(__i).unwrap_or(&__default);"
                ));
                emit_size_expr(k_ty, "__k", w, schema);
                emit_size_expr(v_ty, "__v", w, schema);
                w.dedent();
                w.line("}");
            }
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