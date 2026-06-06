use pojoc_schema::ir::types::*;
use super::types::*;
use super::writer::CodeWriter;

pub fn emit_encode_function(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let latest = schema.versions.last().unwrap();

    // helper write functions for each user-defined type
    emit_type_writers(schema, w);

    w.line(&format!("pub fn encode_payload(buf: &mut Vec<u8>, value: &{name}) {{"));
    w.indent();
    for field in &latest.fields {
        emit_field_write(field, "value", w);
    }
    w.dedent();
    w.line("}");
}

fn emit_type_writers(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let mut latest: std::collections::HashMap<String, (u32, &ResolvedType)> =
        std::collections::HashMap::new();

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
        let fn_name = format!("write_{}", name.to_lowercase());
        w.line(&format!("fn {fn_name}(buf: &mut Vec<u8>, value: &{name}) {{"));
        w.indent();
        for field in &resolved.fields {
            emit_field_write(field, "value", w);
        }
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_field_write(field: &FieldIR, prefix: &str, w: &mut CodeWriter) {
    let accessor = format!("{prefix}.{}", field.name);
    emit_write_expr(&field.ty, &accessor, w);
}

fn emit_write_expr(ty: &ResolvedTypeRef, accessor: &str, w: &mut CodeWriter) {
    match ty {
        ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) => {
            let f = write_fn(&id.name);
            if id.name == "string" {
                w.line(&format!("{f}(buf, &{accessor});"));
            } else {
                w.line(&format!("{f}(buf, {accessor});"));
            }
        }
        ResolvedTypeRef::Scalar(id) => {
            let fn_name = format!("write_{}", id.name.to_lowercase());
            w.line(&format!("{fn_name}(buf, &{accessor});"));
        }
        ResolvedTypeRef::Array(inner) => {
            w.line(&format!("write_array_len(buf, {accessor}.len());"));
            w.line(&format!("for item in &{accessor} {{"));
            w.indent();
            // deref Copy primitives, borrow everything else
            let item_accessor = match inner.as_ref() {
                ResolvedTypeRef::Scalar(id) if is_primitive(&id.name) && id.name != "string" => "*item",
                _ => "item",
            };
            emit_write_expr(inner, item_accessor, w);
            w.dedent();
            w.line("}");
        }
    }
}

pub fn emit_size_hint(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let name = &schema.name_hint;
    let latest = schema.versions.last().unwrap();

    // emit helpers for nested types
    emit_type_size_hints(schema, w);

    w.line(&format!("pub fn size_hint(value: &{name}) -> usize {{"));
    w.indent();
    w.line("let mut size = 5usize; // envelope: 1 version varint + 4 byte length");
    for field in &latest.fields {
        emit_field_size(field, "value", w);
    }
    w.line("size");
    w.dedent();
    w.line("}");
    w.blank();
}

fn emit_type_size_hints(schema: &ResolvedSchema, w: &mut CodeWriter) {
    let mut latest: std::collections::HashMap<String, (u32, &ResolvedType)> =
        std::collections::HashMap::new();

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
        let fn_name = format!("size_hint_{}", name.to_lowercase());
        w.line(&format!("fn {fn_name}(_value: &{name}) -> usize {{"));
        w.indent();
        w.line("let mut size = 0usize;");
        for field in &resolved.fields {
            emit_field_size(field, "value", w);
        }
        w.line("size");
        w.dedent();
        w.line("}");
        w.blank();
    }
}

fn emit_field_size(field: &FieldIR, prefix: &str, w: &mut CodeWriter) {
    let accessor = format!("{prefix}.{}", field.name);
    emit_size_expr(&field.ty, &accessor, w);
}

fn emit_size_expr(ty: &ResolvedTypeRef, accessor: &str, w: &mut CodeWriter) {
    match ty {
        ResolvedTypeRef::Scalar(id) => {
            let size = scalar_size(&id.name, accessor);
            w.line(&format!("size += {size};"));
        }
        ResolvedTypeRef::Array(inner) => {
            if let ResolvedTypeRef::Scalar(id) = inner.as_ref() {
                if is_fixed_size(&id.name) {
                    let elem_size = fixed_scalar_size(&id.name);
                    w.line(&format!("size += varint_size({accessor}.len());"));
                    w.line(&format!("size += {accessor}.len() * {elem_size};"));
                    return;
                }
            }
            w.line(&format!("size += varint_size({accessor}.len());"));
            w.line(&format!("for item in &{accessor} {{"));
            w.indent();
            emit_size_expr(inner, "item", w);
            w.dedent();
            w.line("}");
        }
    }
}

fn is_fixed_size(name: &str) -> bool {
    matches!(normalize_type(name), 
            "bool" | "u8" | "i8" | "u16" | "i16" | 
            "u32" | "i32" | "f32" | "u64" | "i64" | "f64")
}

fn scalar_size(name: &str, accessor: &str) -> String {
    match normalize_type(name) {
        "bool" => "1".to_string(),
        "u8" | "i8" => "1".to_string(),
        "u16" | "i16" => "2".to_string(),
        "u32" | "i32" | "f32" => "4".to_string(),
        "u64" | "i64" | "f64" => "8".to_string(),
        "string" => format!("varint_size({accessor}.len()) + {accessor}.len()"),
        other => format!("size_hint_{}(&{accessor})", other.to_lowercase()),
    }
}

fn fixed_scalar_size(name: &str) -> usize {
    match normalize_type(name) {
        "bool" | "u8" | "i8" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        "u64" | "i64" | "f64" => 8,
        _ => unreachable!(),
    }
}