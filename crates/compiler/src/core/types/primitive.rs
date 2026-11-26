use super::{WireSize, normalize_type};

pub(crate) struct PrimitiveInfo {
    pub wire_size: WireSize,
    pub rust_type: &'static str,
    pub rust_default: &'static str,
    pub typescript_type: &'static str,
    pub typescript_array: Option<&'static str>,
    pub typescript_reader: Option<&'static str>,
    pub typescript_writer: &'static str,
}

#[rustfmt::skip]
pub(crate) fn primitive_info(name: &str) -> Option<PrimitiveInfo> {
    use WireSize::{Fixed, Variable};

    //                       Rust type    TS type    Rust default   Wire size   TS packed array          TS DataView reader      TS writer
    let (rust_type, typescript_type, rust_default, wire_size, typescript_array, typescript_reader, typescript_writer) =
        match normalize_type(name) {
            "u8"       => ("u8",       "number",   "0u8",         Fixed(1),   Some("Uint8Array"),     Some("getUint8"),       "writeU8"),
            "i8"       => ("i8",       "number",   "0i8",         Fixed(1),   Some("Int8Array"),      Some("getInt8"),        "writeI8"),
            "u16"      => ("u16",      "number",   "0u16",        Fixed(2),   Some("Uint16Array"),    Some("getUint16"),      "writeU16"),
            "i16"      => ("i16",      "number",   "0i16",        Fixed(2),   Some("Int16Array"),     Some("getInt16"),       "writeI16"),
            "u32"      => ("u32",      "number",   "0u32",        Fixed(4),   Some("Uint32Array"),    Some("getUint32"),      "writeU32"),
            "i32"      => ("i32",      "number",   "0i32",        Fixed(4),   Some("Int32Array"),     Some("getInt32"),       "writeI32"),
            "u64"      => ("u64",      "bigint",   "0u64",        Fixed(8),   Some("BigUint64Array"), Some("getBigUint64"),   "writeU64"),
            "i64"      => ("i64",      "bigint",   "0i64",        Fixed(8),   Some("BigInt64Array"),  Some("getBigInt64"),    "writeI64"),
            "f32"      => ("f32",      "number",   "0.0f32",      Fixed(4),   Some("Float32Array"),   Some("getFloat32"),     "writeF32"),
            "f64"      => ("f64",      "number",   "0.0f64",      Fixed(8),   Some("Float64Array"),   Some("getFloat64"),     "writeF64"),
            "bool"     => ("bool",     "boolean",  "false",       Fixed(1),   None,                  Some("getUint8"),        "writeBool"),
            "varint32" => ("u32",      "number",   "0u32",        Variable,   None,                  None,                    "writeVarint32Value"),
            "varint64" => ("u64",      "bigint",   "0u64",        Variable,   None,                  None,                    "writeVarint64Value"),
            "string"   => ("&'buf str","string",   "\"\"",        Variable,   None,                  None,                    "writeString"),
            _ => return None,
        };
    Some(PrimitiveInfo {
        wire_size,
        rust_type,
        rust_default,
        typescript_type,
        typescript_array,
        typescript_reader,
        typescript_writer,
    })
}
