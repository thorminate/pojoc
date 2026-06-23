use heck::ToSnakeCase;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeId {
    pub name: String,
    pub version: i128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VFloatBacking {
    U16,
    U32,
}

impl VFloatBacking {
    pub fn rust_int_type(&self) -> &'static str {
        match self {
            VFloatBacking::U16 => "u16",
            VFloatBacking::U32 => "u32",
        }
    }
    pub fn wire_size(&self) -> usize {
        match self {
            VFloatBacking::U16 => 2,
            VFloatBacking::U32 => 4,
        }
    }
    pub fn read_fn(&self) -> &'static str {
        match self {
            VFloatBacking::U16 => "read_u16",
            VFloatBacking::U32 => "read_u32",
        }
    }
    pub fn write_fn(&self) -> &'static str {
        match self {
            VFloatBacking::U16 => "write_u16",
            VFloatBacking::U32 => "write_u32",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedTypeRef {
    Scalar(TypeId),
    Enum(TypeId),
    Bitset(TypeId, u8),
    Union(TypeId),
    Array(Box<ResolvedTypeRef>),
    FixedArray(Box<ResolvedTypeRef>, usize),
    DeltaArray(Box<ResolvedTypeRef>),
    FixedDeltaArray(Box<ResolvedTypeRef>, usize),
    FixedString(usize),
    Map(Box<ResolvedTypeRef>, Box<ResolvedTypeRef>),
    FixedMap(Box<ResolvedTypeRef>, Box<ResolvedTypeRef>, usize),
    Tuple(Vec<ResolvedTypeRef>),
    VFloat {
        min: f64,
        max: f64,
        step: f64,
        backing: VFloatBacking,
    },
    Optional(Box<ResolvedTypeRef>),
    ImportedSchema {
        alias: String,
        root_name: String,
        version: i128,
    },
}

impl ResolvedTypeRef {
    pub fn type_id(&self) -> Option<&TypeId> {
        match self {
            ResolvedTypeRef::Scalar(id) => Option::from(id),
            ResolvedTypeRef::Enum(id) => Some(id),
            ResolvedTypeRef::Union(id) => Some(id),
            ResolvedTypeRef::Bitset(id, _) => Some(id),
            ResolvedTypeRef::Array(id) => id.type_id(),
            ResolvedTypeRef::FixedArray(inner, _) => inner.type_id(),
            ResolvedTypeRef::DeltaArray(inner) => inner.type_id(),
            ResolvedTypeRef::FixedDeltaArray(inner, _) => inner.type_id(),
            ResolvedTypeRef::FixedString(_) => None,
            ResolvedTypeRef::Map(_, v) => v.type_id(),
            ResolvedTypeRef::FixedMap(_, v, _) => v.type_id(),
            ResolvedTypeRef::Tuple(_) => None,
            ResolvedTypeRef::VFloat { .. } => None,
            ResolvedTypeRef::Optional(v) => v.type_id(),
            ResolvedTypeRef::ImportedSchema { .. } => None,
        }
    }
}

pub fn normalize_type(name: &str) -> &str {
    match name {
        "byte" | "uchar" | "u8" => "u8",
        "ushort" | "u16" => "u16",
        "uint" | "u32" => "u32",
        "ulong" | "u64" => "u64",
        "char" | "i8" => "i8",
        "short" | "i16" => "i16",
        "int" | "i32" => "i32",
        "long" | "i64" => "i64",
        "float" | "f32" => "f32",
        "double" | "f64" => "f64",
        "varint32" => "varint32",
        "varint64" => "varint64",
        "bool" | "boolean" => "bool",
        "string" | "str" => "string",
        other => other,
    }
}

pub fn is_primitive(name: &str) -> bool {
    matches!(
        normalize_type(name),
        "u8" | "u16"
            | "u32"
            | "u64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
            | "varint32"
            | "varint64"
            | "bool"
            | "string"
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WireSize {
    Fixed(usize),
    Variable,
}

pub struct TypeInfo {
    pub wire_size: WireSize,
    pub rust_type: String,
    pub skip_stmt: String,
    pub read_fn: String,
    pub write_fn: String,
    pub default_expr: String,
    pub size_fn: Option<String>,
}

impl TypeInfo {
    pub fn size_expr(&self, accessor: &str) -> String {
        match self.wire_size {
            WireSize::Fixed(n) => n.to_string(),
            WireSize::Variable => {
                if self.rust_type == "PojocString" {
                    format!("varint_size({accessor}.len()) + {accessor}.len()")
                } else if let Some(ref f) = self.size_fn {
                    format!("{f}({accessor} as usize)")
                } else {
                    format!("size_hint_{}(&{accessor})", self.rust_type.to_snake_case())
                }
            }
        }
    }
}

pub fn type_info(ty: &ResolvedTypeRef) -> TypeInfo {
    match ty {
        ResolvedTypeRef::Scalar(id) => scalar_info(&id.name),

        ResolvedTypeRef::Enum(id) => TypeInfo {
            wire_size: WireSize::Variable,
            rust_type: id.name.clone(),
            skip_stmt: "{ let _ = read_varint32(buf, pos)?; }".into(),
            read_fn: format!("read_enum::<{}>", id.name),
            write_fn: "encode_varint".into(),
            default_expr: format!("{}::default()", id.name),
            size_fn: None,
        },

        ResolvedTypeRef::Union(id) => {
            let lower = id.name.to_snake_case();
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: id.name.clone(),
                skip_stmt: format!("skip_{lower}(buf, pos)?;"),
                read_fn: format!("read_{lower}"),
                write_fn: format!("write_{lower}"),
                default_expr: format!("{}::default()", id.name),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Bitset(id, width) => {
            let lower = id.name.to_snake_case();
            let (skip, wire) = match width {
                1 => ("let _ = read_u8(buf, pos)?;", WireSize::Fixed(1)),
                2 => ("let _ = read_u16(buf, pos)?;", WireSize::Fixed(2)),
                _ => ("let _ = read_u32(buf, pos)?;", WireSize::Fixed(4)),
            };
            TypeInfo {
                wire_size: wire,
                rust_type: id.name.clone(),
                skip_stmt: skip.into(),
                read_fn: format!("read_{lower}"),
                write_fn: format!("write_{lower}"),
                default_expr: format!("{}::default()", id.name),
                size_fn: None,
            }
        }

        ResolvedTypeRef::FixedString(n) => TypeInfo {
            wire_size: WireSize::Fixed(*n),
            rust_type: format!("[u8; {n}]"),
            skip_stmt: format!(
                "{{ let __end = pos.checked_add({n}).ok_or(Error::InvalidLength)?; \
                 if __end > buf.len() {{ return Err(Error::InvalidLength); }} *pos = __end; }}"
            ),
            read_fn: format!("read_fixed_bytes::<{n}>"),
            write_fn: format!("write_fixed_bytes::<{n}>"),
            default_expr: format!("[0u8; {n}]"),
            size_fn: None,
        },

        ResolvedTypeRef::FixedArray(inner, n) => {
            let i = type_info(inner);
            TypeInfo {
                wire_size: match i.wire_size {
                    WireSize::Fixed(s) => WireSize::Fixed(s * n),
                    WireSize::Variable => WireSize::Variable,
                },
                rust_type: format!("[{}; {n}]", i.rust_type),
                skip_stmt: format!("for _ in 0..{n} {{ {} }}", i.skip_stmt),
                read_fn: format!("read_fixed_array::<{n}>"),
                write_fn: format!("write_fixed_array::<{n}>"),
                default_expr: "std::array::from_fn(|_| Default::default())".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Array(inner) => {
            let i = type_info(inner);
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!("PojocVec<{}>", i.rust_type),
                skip_stmt: format!(
                    "{{ let __n = read_array_len(buf, pos)?; for _ in 0..__n {{ {} }} }}",
                    i.skip_stmt
                ),
                read_fn: "read_array".into(),
                write_fn: "write_array".into(),
                default_expr: "PojocVec::new()".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::DeltaArray(inner) => {
            let i = type_info(inner);
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!("PojocVec<{}>", i.rust_type),
                skip_stmt: format!("skip_delta_array::<{}>(buf, pos)?;", i.rust_type),
                read_fn: format!("read_delta_array::<{}>", i.rust_type),
                write_fn: format!("write_delta_array::<{}>", i.rust_type),
                default_expr: "PojocVec::new()".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::FixedDeltaArray(inner, n) => {
            let i = type_info(inner);
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!("[{}; {n}]", i.rust_type),
                skip_stmt: format!("skip_fixed_delta_array::<{}, {n}>(buf, pos)?;", i.rust_type),
                read_fn: format!("read_fixed_delta_array::<{}, {n}>", i.rust_type),
                write_fn: format!("write_fixed_delta_array::<{}, {n}>", i.rust_type),
                default_expr: "std::array::from_fn(|_| Default::default())".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Map(k, v) => {
            let (ki, vi) = (type_info(k), type_info(v));
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!("PojocMap<{}, {}>", ki.rust_type, vi.rust_type),
                skip_stmt: format!(
                    "{{ let __n = read_array_len(buf, pos)?; for _ in 0..__n {{ {} {} }} }}",
                    ki.skip_stmt, vi.skip_stmt
                ),
                read_fn: "/* map expressions are inlined */".into(),
                write_fn: "/* map expressions are inlined */".into(),
                default_expr: "PojocMap::new()".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::FixedMap(k, v, n) => {
            let (ki, vi) = (type_info(k), type_info(v));
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!("PojocFixedMap<{}, {}, {n}>", ki.rust_type, vi.rust_type),
                skip_stmt: format!("for _ in 0..{n} {{ {} {} }}", ki.skip_stmt, vi.skip_stmt),
                read_fn: format!("read_fixed_map::<{n}>"),
                write_fn: format!("write_fixed_map::<{n}>"),
                default_expr: "PojocFixedMap::new()".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Tuple(elems) => {
            let infos: Vec<_> = elems.iter().map(type_info).collect();
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!(
                    "({})",
                    infos
                        .iter()
                        .map(|i| i.rust_type.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                skip_stmt: infos
                    .iter()
                    .map(|i| i.skip_stmt.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                read_fn: "read_tuple".into(),
                write_fn: "write_tuple".into(),
                default_expr: format!(
                    "({})",
                    infos
                        .iter()
                        .map(|i| i.default_expr.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                size_fn: None,
            }
        }

        ResolvedTypeRef::VFloat { min, backing, .. } => TypeInfo {
            wire_size: WireSize::Fixed(backing.wire_size()),
            rust_type: "f32".into(),
            skip_stmt: format!("let _ = {}(buf, pos)?;", backing.read_fn()),
            read_fn: backing.read_fn().into(),
            write_fn: backing.write_fn().into(),
            default_expr: format!("{}f32", *min as f32),
            size_fn: None,
        },

        ResolvedTypeRef::Optional(inner) => {
            let i = type_info(inner);
            let inner_stmt = strip_redundant_braces(&i.skip_stmt);
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!("Option<{}>", i.rust_type),
                skip_stmt: format!("if read_u8(buf, pos)? != 0 {{ {inner_stmt} }}"),
                read_fn: "/* optional expressions are handled inline */".into(),
                write_fn: "/* optional expressions are handled inline */".into(),
                default_expr: "None".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::ImportedSchema {
            alias,
            root_name,
            version,
        } => {
            let module = alias.to_snake_case();
            let rust_type = format!("{module}::{root_name}");
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: rust_type.clone(),
                skip_stmt: format!("{module}::skip_v{version}(buf, pos)?;"),
                read_fn: format!("{module}::decode_v{version}"),
                write_fn: format!("{module}::encode_v{version}"),
                default_expr: format!("{rust_type}::default()"),
                size_fn: None,
            }
        }
    }
}

fn scalar_info(name: &str) -> TypeInfo {
    macro_rules! fixed {
        ($size:expr, $rust:expr, $read:expr, $write:expr, $default:expr) => {
            TypeInfo {
                wire_size: WireSize::Fixed($size),
                rust_type: $rust.into(),
                skip_stmt: format!("let _ = {}(buf, pos)?;", $read),
                read_fn: $read.into(),
                write_fn: $write.into(),
                default_expr: $default.into(),
                size_fn: None,
            }
        };
    }

    match normalize_type(name) {
        "u8" => fixed!(1, "u8", "read_u8", "write_u8", "0u8"),
        "i8" => fixed!(1, "i8", "read_i8", "write_i8", "0i8"),
        "bool" => fixed!(1, "bool", "read_bool", "write_bool", "false"),
        "u16" => fixed!(2, "u16", "read_u16", "write_u16", "0u16"),
        "i16" => fixed!(2, "i16", "read_i16", "write_i16", "0i16"),
        "u32" => fixed!(4, "u32", "read_u32", "write_u32", "0u32"),
        "i32" => fixed!(4, "i32", "read_i32", "write_i32", "0i32"),
        "f32" => fixed!(4, "f32", "read_f32", "write_f32", "0.0f32"),
        "u64" => fixed!(8, "u64", "read_u64", "write_u64", "0u64"),
        "i64" => fixed!(8, "i64", "read_i64", "write_i64", "0i64"),
        "f64" => fixed!(8, "f64", "read_f64", "write_f64", "0.0f64"),
        "varint32" => TypeInfo {
            wire_size: WireSize::Variable,
            rust_type: "u32".into(),
            skip_stmt: "skip_varint32(buf, pos)?;".into(),
            read_fn: "read_varint32".into(),
            write_fn: "write_varint32".into(),
            default_expr: "0u32".into(),
            size_fn: Some("varint_size".into()),
        },
        "varint64" => TypeInfo {
            wire_size: WireSize::Variable,
            rust_type: "u64".into(),
            skip_stmt: "skip_varint64(buf, pos)?;".into(),
            read_fn: "read_varint64".into(),
            write_fn: "write_varint64".into(),
            default_expr: "0u64".into(),
            size_fn: Some("varint_size".into()),
        },
        "string" => TypeInfo {
            wire_size: WireSize::Variable,
            rust_type: "PojocString".into(),
            skip_stmt: "skip_string(buf, pos)?;".into(),
            read_fn: "read_pojoc_string".into(),
            write_fn: "write_pojoc_string".into(),
            default_expr: "PojocString::default()".into(),
            size_fn: None,
        },
        other => {
            let lower = other.to_snake_case();
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: other.into(),
                skip_stmt: format!("skip_{lower}(buf, pos)?;"),
                read_fn: format!("read_{lower}"),
                write_fn: format!("write_{lower}"),
                default_expr: "Default::default()".into(),
                size_fn: None,
            }
        }
    }
}

fn strip_redundant_braces(s: &str) -> &str {
    let s = s.trim();
    if s.starts_with('{') && s.ends_with('}') {
        let inner = &s[1..s.len() - 1];
        if is_balanced(inner) {
            return inner.trim();
        }
    }
    s
}

fn is_balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

pub fn is_delta_eligible(ty: &ResolvedTypeRef) -> bool {
    matches!(
        ty,
        ResolvedTypeRef::Scalar(id) if is_delta_eligible_str(normalize_type(&id.name))
    )
}

pub fn is_delta_eligible_str(str: &str) -> bool {
    matches!(
        str,
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64"
    )
}
