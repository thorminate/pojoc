use pojoc_schema::ir::types::{ResolvedTypeRef};

pub fn normalize_type(name: &str) -> &str {
    match name {
        // unsigned
        "byte" | "uchar" | "u8" => "u8",
        "ushort" | "u16" => "u16",
        "uint" | "u32" => "u32",
        "ulong" | "u64" => "u64",

        // signed
        "char" | "i8" => "i8",
        "short" | "i16" => "i16",
        "int" | "i32" => "i32",
        "long" | "i64" => "i64",

        // floats
        "float" | "f32" => "f32",
        "double" | "f64" => "f64",

        // bool
        "bool" | "boolean" => "bool",

        // strings
        "string" | "str" => "string",

        // bytes
        "bytes" | "blob" | "binary" => "bytes",

        // already canonical
        other => other,
    }
}

pub fn is_primitive(name: &str) -> bool {
    matches!(normalize_type(name),
        "u8" | "u16" | "u32" | "u64" |
        "i8" | "i16" | "i32" | "i64" |
        "f32" | "f64" |
        "string" | "bool"
    )
}

pub fn rust_scalar_type(name: &str) -> &'static str {
    match normalize_type(name) {
        "u32" => "u32",
        "u8" => "u8",
        "u16" => "u16",
        "u64" => "u64",

        "i32" => "i32",
        "i8" => "i8",
        "i16" => "i16",
        "i64" => "i64",

        "f32" => "f32",
        "f64" => "f64",

        "string" => "PojocString",
        "bytes" => "PojocVec<u8>",
        "bool" => "bool",

        _ => "/* unknown */",
    }
}

pub fn rust_field_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Scalar(id) => {
            if is_primitive(&id.name) {
                rust_scalar_type(&id.name).to_string()
            } else {
                id.name.clone()
            }
        }
        ResolvedTypeRef::Array(inner) => {
            format!("PojocVec<{}>", rust_field_type(inner))
        }
    }
}

pub fn read_call(name: &str) -> &'static str {
    match normalize_type(name) {
        "u32" => "read_u32(buf, pos)?",
        "u8" => "read_u8(buf, pos)?",
        "u16" => "read_u16(buf, pos)?",
        "u64" => "read_u64(buf, pos)?",

        "i32" => "read_i32(buf, pos)?",
        "i8" => "read_i8(buf, pos)?",
        "i16" => "read_i16(buf, pos)?",
        "i64" => "read_i64(buf, pos)?",

        "f32" => "read_f32(buf, pos)?",
        "f64" => "read_f64(buf, pos)?",

        "string" => "read_pojoc_string(buf, pos)?",
        "bytes" => "read_bytes(buf, pos)?",
        "bool" => "read_bool(buf, pos)?",

        _ => "/* unknown read */",
    }
}

pub fn write_fn(name: &str) -> &'static str {
    match normalize_type(name) {
        "u32" => "write_u32",
        "u8" => "write_u8",
        "u16" => "write_u16",
        "u64" => "write_u64",

        "i32" => "write_i32",
        "i8" => "write_i8",
        "i16" => "write_i16",
        "i64" => "write_i64",

        "f32" => "write_f32",
        "f64" => "write_f64",

        "string" => "write_pojoc_string",
        "bytes" => "write_bytes",
        "bool" => "write_bool",

        _ => "/* unknown write */",
    }
}

pub fn default_value(ty: &ResolvedTypeRef) -> &'static str {
    match ty {
        ResolvedTypeRef::Array(_) => "PojocVec::new()",

        ResolvedTypeRef::Scalar(id) => match normalize_type(&id.name) {
            "string" => "PojocString::new()",

            "f32" => "0.0f32",
            "f64" => "0.0f64",

            "i8" => "0i8",
            "i16" => "0i16",
            "i32" => "0i32",
            "i64" => "0i64",

            "u8" => "0u8",
            "u16" => "0u16",
            "u32" => "0u32",
            "u64" => "0u64",

            "bool" => "false",

            _ => "Default::default()",
        },
    }
}