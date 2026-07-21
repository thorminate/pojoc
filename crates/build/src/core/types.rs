use heck::ToSnakeCase;
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    /// Names of lifetime-infected types (structs that carry a `<'buf>` because
    /// they transitively hold a borrowed `&'buf str` or a `lazy` field), plus
    /// module-qualified imported roots (`"player::Player"`) that are infected.
    /// Set once per [`crate::codegen::generate`] invocation; read by
    /// [`type_info`] so infected type names render with `<'buf>` *everywhere*
    /// they nest — inside arrays, maps, tuples, generics, and imports — not just
    /// as a direct struct field. Codegen is single-threaded per `generate`.
    static INFECTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());

    /// Names of constraint-infected types (structs with a `min:`/`max:`
    /// constrained field, directly or transitively). Mirrors `INFECTED`, but
    /// drives a different codegen decision: an infected type's `write_*`/
    /// `encode_vN` function returns `PojocResult<()>` instead of `()`, so
    /// callers of an infected named type's writer need a `?` appended.
    static CONSTRAINT_INFECTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());

    /// Names of intern-infected types (structs with an `intern`-marked field,
    /// directly or transitively). An infected type's `write_*`/`read_*`
    /// function gains an extra `__interner`/`__table` parameter threading the
    /// message's shared string table, so callers need to pass it through.
    static INTERN_INFECTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Install the set of lifetime-infected type names for subsequent code
/// generation on this thread. Returns the previous set so callers can restore
/// it around nested `generate` calls (imported submodules).
pub fn set_infected(names: HashSet<String>) -> HashSet<String> {
    INFECTED.with(|c| c.replace(names))
}

/// Whether `name` is a lifetime-infected type in the current generation.
pub fn is_infected(name: &str) -> bool {
    INFECTED.with(|c| c.borrow().contains(name))
}

/// Install the set of constraint-infected type names for subsequent code
/// generation on this thread. Returns the previous set, same restore
/// convention as [`set_infected`].
pub fn set_constraint_infected(names: HashSet<String>) -> HashSet<String> {
    CONSTRAINT_INFECTED.with(|c| c.replace(names))
}

/// Whether `name`'s `write_*`/`encode_vN` function returns `PojocResult<()>`
/// in the current generation (it has, or transitively contains, a
/// `min:`/`max:` constrained field).
pub fn is_constraint_infected(name: &str) -> bool {
    CONSTRAINT_INFECTED.with(|c| c.borrow().contains(name))
}

/// Install the set of intern-infected type names for subsequent code
/// generation on this thread. Returns the previous set, same restore
/// convention as [`set_infected`].
pub fn set_intern_infected(names: HashSet<String>) -> HashSet<String> {
    INTERN_INFECTED.with(|c| c.replace(names))
}

/// Whether `name`'s `write_*`/`read_*` function threads the shared
/// string-interning table (`__interner`/`__table`) in the current generation.
pub fn is_intern_infected(name: &str) -> bool {
    INTERN_INFECTED.with(|c| c.borrow().contains(name))
}

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
    /// `box<T>` — heap indirection, the only way to break a self-referential
    /// or mutually-recursive struct cycle (see the cycle-validation pass in
    /// the analyzer, which rejects any cycle that doesn't cross a `Boxed`).
    Boxed(Box<ResolvedTypeRef>),
    /// A `(min:, max:)` validation constraint wrapping a number, string,
    /// array, or map type — enforced on both encode and decode. Doesn't
    /// change the wire format at all (identical to the unwrapped inner
    /// type), only adds a generated range/length/count check.
    Constrained {
        inner: Box<ResolvedTypeRef>,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// `intern <type>` — opt-in string deduplication. Always wraps a bare
    /// `string` scalar (enforced at resolve time); composes anywhere in the
    /// type tree (array/map elements, tuple elements, generic args), not
    /// just at a field's own top level. Doesn't change the Rust type
    /// (`&'buf str`, same as plain `string`) — only the wire representation
    /// (a table index instead of inline bytes) and the read/write codegen.
    Interned(Box<ResolvedTypeRef>),
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
            ResolvedTypeRef::Boxed(v) => v.type_id(),
            ResolvedTypeRef::Constrained { inner, .. } => inner.type_id(),
            ResolvedTypeRef::Interned(inner) => inner.type_id(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub fn size_expr(&self, is_ref: bool, accessor: &str) -> String {
        match self.wire_size {
            WireSize::Fixed(n) => n.to_string(),
            WireSize::Variable => {
                if self.rust_type == "PojocString" || self.rust_type == "&'buf str" {
                    format!("varint_size({accessor}.len()) + {accessor}.len()")
                } else if let Some(ref f) = self.size_fn {
                    format!("{f}({accessor} as usize)")
                } else {
                    let borrow_symbol = if is_ref { "" } else { "&" };
                    // Strip any `<'buf>` before deriving the helper name so an
                    // infected `NestedLeaf<'buf>` maps to `size_hint_nested_leaf`.
                    let base = self.rust_type.split('<').next().unwrap_or(&self.rust_type);
                    format!(
                        "size_hint_{}({borrow_symbol}{accessor})",
                        base.to_snake_case()
                    )
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
            skip_stmt: "{ let _ = read_varint32(__buf, __pos)?; }".into(),
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
                skip_stmt: format!("skip_{lower}(__buf, __pos)?;"),
                read_fn: format!("read_{lower}"),
                write_fn: format!("write_{lower}"),
                default_expr: format!("{}::default()", id.name),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Bitset(id, width) => {
            let lower = id.name.to_snake_case();
            let (skip, wire) = match width {
                1 => ("let _ = read_u8(__buf, __pos)?;", WireSize::Fixed(1)),
                2 => ("let _ = read_u16(__buf, __pos)?;", WireSize::Fixed(2)),
                _ => ("let _ = read_u32(__buf, __pos)?;", WireSize::Fixed(4)),
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
                "{{ let __end = __pos.checked_add({n}).ok_or(Error::InvalidLength)?; \
                 if __end > __buf.len() {{ return Err(Error::InvalidLength); }} *__pos = __end; }}"
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
                    "{{ let __n = read_array_len(__buf, __pos)?; for _ in 0..__n {{ {} }} }}",
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
                skip_stmt: format!("skip_delta_array::<{}>(__buf, __pos)?;", i.rust_type),
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
                skip_stmt: format!(
                    "skip_fixed_delta_array::<{}, {n}>(__buf, __pos)?;",
                    i.rust_type
                ),
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
                    "{{ let __n = read_array_len(__buf, __pos)?; for _ in 0..__n {{ {} {} }} }}",
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
                wire_size: if let WireSize::Fixed(k) = ki.wire_size {
                    if let WireSize::Fixed(v) = vi.wire_size {
                        WireSize::Fixed(k * v * n)
                    } else {
                        WireSize::Variable
                    }
                } else {
                    WireSize::Variable
                },
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
            // A tuple whose every element is a fixed-width, `Copy` scalar-like type
            // is itself fixed-width. This lets the codegen group tuple fields (e.g.
            // `(f32, f32, f32)` positions/velocities) into the single-bounds-check
            // fixed block. FixedMap is fixed-size but not `Copy`, so tuples that
            // contain one stay `Variable` to avoid an invalid `[default; N]` init.
            let tuple_wire_size = {
                let mut total = 0usize;
                let mut all_fixed = true;
                for (elem, info) in elems.iter().zip(&infos) {
                    match info.wire_size {
                        WireSize::Fixed(n) if !contains_fixed_map(elem) => total += n,
                        _ => {
                            all_fixed = false;
                            break;
                        }
                    }
                }
                if all_fixed {
                    WireSize::Fixed(total)
                } else {
                    WireSize::Variable
                }
            };
            TypeInfo {
                wire_size: tuple_wire_size,
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
            skip_stmt: format!("let _ = {}(__buf, __pos)?;", backing.read_fn()),
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
                skip_stmt: format!("if read_u8(__buf, __pos)? != 0 {{ {inner_stmt} }}"),
                read_fn: "/* optional expressions are handled inline */".into(),
                write_fn: "/* optional expressions are handled inline */".into(),
                default_expr: "None".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Boxed(inner) => {
            let i = type_info(inner);
            TypeInfo {
                // Heap indirection doesn't change the wire representation —
                // the bytes on the wire are exactly the inner value's bytes,
                // it's only the in-memory Rust field that gains a pointer.
                wire_size: i.wire_size,
                rust_type: format!("Box<{}>", i.rust_type),
                skip_stmt: i.skip_stmt.clone(),
                read_fn: "/* boxed expressions are handled inline */".into(),
                write_fn: "/* boxed expressions are handled inline */".into(),
                default_expr: format!("Box::new({})", i.default_expr),
                size_fn: None,
            }
        }

        // A constraint is purely a validation-time check — it doesn't change
        // the Rust field type or the wire format at all, so it's transparent
        // to `type_info`. Codegen inspects the constraint bounds separately
        // (see `compute_constraint_infected` and the guard emission in
        // `codegen/encode.rs`/`codegen/decode.rs`) wherever it needs them.
        ResolvedTypeRef::Constrained { inner, .. } => type_info(inner),

        ResolvedTypeRef::Interned(inner) => {
            let i = type_info(inner);
            TypeInfo {
                wire_size: WireSize::Variable,
                // Same in-memory type as plain `string` — interning only
                // changes what's on the wire (a table index), not what the
                // decoded value looks like in Rust.
                rust_type: i.rust_type,
                // Skipping an interned field skips just the index varint —
                // the actual string bytes already live in the table, read
                // once up front, not inline at this field's position.
                skip_stmt: "skip_varint64(__buf, __pos)?;".into(),
                read_fn: "/* interned expressions are handled inline */".into(),
                write_fn: "/* interned expressions are handled inline */".into(),
                default_expr: i.default_expr,
                size_fn: None,
            }
        }

        ResolvedTypeRef::ImportedSchema {
            alias,
            root_name,
            version,
        } => {
            let module = alias.to_snake_case();
            // An imported root that is itself lifetime-infected (e.g. it now
            // holds borrowed strings) is tracked in `INFECTED` under its
            // module-qualified name, so the field type carries `<'buf>`.
            let qualified = format!("{module}::{root_name}");
            let rust_type = if is_infected(&qualified) {
                format!("{qualified}<'buf>")
            } else {
                qualified.clone()
            };
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: rust_type.clone(),
                skip_stmt: format!("{module}::skip_v{version}(__buf, __pos)?;"),
                read_fn: format!("{module}::decode_v{version}"),
                write_fn: format!("{module}::encode_v{version}"),
                default_expr: format!("{qualified}::default()"),
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
                skip_stmt: format!("let _ = {}(__buf, __pos)?;", $read),
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
            skip_stmt: "skip_varint32(__buf, __pos)?;".into(),
            read_fn: "read_varint32".into(),
            write_fn: "write_varint32".into(),
            default_expr: "0u32".into(),
            size_fn: Some("varint_size".into()),
        },
        "varint64" => TypeInfo {
            wire_size: WireSize::Variable,
            rust_type: "u64".into(),
            skip_stmt: "skip_varint64(__buf, __pos)?;".into(),
            read_fn: "read_varint64".into(),
            write_fn: "write_varint64".into(),
            default_expr: "0u64".into(),
            size_fn: Some("varint_size".into()),
        },
        "string" => TypeInfo {
            wire_size: WireSize::Variable,
            rust_type: "&'buf str".into(),
            skip_stmt: "skip_string(__buf, __pos)?;".into(),
            read_fn: "read_string".into(),
            write_fn: "write_string".into(),
            default_expr: "\"\"".into(),
            size_fn: None,
        },
        other => {
            let lower = other.to_snake_case();
            // A named struct that is lifetime-infected renders with `<'buf>`
            // wherever it appears — as a field, or nested inside an array/map/
            // tuple/generic. Helper fn names (`read_*`) stay lifetime-free; the
            // lifetime is inferred at the call site.
            let rust_type = if is_infected(other) {
                format!("{other}<'buf>")
            } else {
                other.into()
            };
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type,
                skip_stmt: format!("skip_{lower}(__buf, __pos)?;"),
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

/// Whether `ty` is, or transitively contains, a `FixedMap` or `Boxed` — the
/// fixed-width types whose Rust representation (`PojocFixedMap`, `Box<T>`) is
/// not `Copy`. Used to keep such tuples/fields out of the `Copy`-requiring
/// fixed-array/fixed-block init paths, even when their wire size is fixed.
pub fn contains_fixed_map(ty: &ResolvedTypeRef) -> bool {
    match ty {
        ResolvedTypeRef::FixedMap(..) => true,
        ResolvedTypeRef::Boxed(..) => true,
        ResolvedTypeRef::FixedArray(inner, _) => contains_fixed_map(inner),
        ResolvedTypeRef::Tuple(elems) => elems.iter().any(contains_fixed_map),
        _ => false,
    }
}

/// Whether `ty` is, or transitively contains anywhere (array/map/tuple
/// elements, `Optional`/`Boxed`/`Constrained` wrapping), an `Interned` type.
pub fn contains_interned(ty: &ResolvedTypeRef) -> bool {
    match ty {
        ResolvedTypeRef::Interned(_) => true,
        ResolvedTypeRef::Array(inner)
        | ResolvedTypeRef::FixedArray(inner, _)
        | ResolvedTypeRef::DeltaArray(inner)
        | ResolvedTypeRef::FixedDeltaArray(inner, _)
        | ResolvedTypeRef::Optional(inner)
        | ResolvedTypeRef::Boxed(inner)
        | ResolvedTypeRef::Constrained { inner, .. } => contains_interned(inner),
        ResolvedTypeRef::Tuple(elems) => elems.iter().any(contains_interned),
        ResolvedTypeRef::Map(k, v) | ResolvedTypeRef::FixedMap(k, v, _) => {
            contains_interned(k) || contains_interned(v)
        }
        _ => false,
    }
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
