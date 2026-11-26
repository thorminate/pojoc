use heck::ToSnakeCase;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

thread_local! {
    // infected type names render with <'buf> everywhere they nest (arrays,
    // maps, tuples, generics, imports), not just as a direct struct field.
    // set once per generate() call, single-threaded per call.
    static INFECTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());

    // mirrors INFECTED, but for a different codegen decision: an infected
    // type's write_*/encode_vN returns PojocResult<()> instead of (), so
    // callers need a `?` appended.
    static CONSTRAINT_INFECTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());

    // mirrors INFECTED; an infected type's write_*/read_* gains an extra
    // __interner/__table param threading the shared string table.
    static INTERN_INFECTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());

    // unlike INTERN_INFECTED (blanket by name, fine for named structs which
    // only ever have one nested read_foo/write_foo), a union gets one codec
    // function *per resolved version* (read_foo_vN/write_foo_vN), each frozen
    // at that version's own variant set — so whether a specific version's
    // codec needs the intern-table param is tracked per TypeId (name+version),
    // not by name alone. Mirrors the "don't retroactively add a table to an
    // old wire version" precision version_uses_intern already has for the
    // root schema. This is the *read* side's criterion: read_foo_vN only ever
    // needs to decode discriminants that version's own historical variant set
    // could have produced.
    static UNION_INTERN_INFECTED: RefCell<HashSet<TypeId>> = RefCell::new(HashSet::new());

    // the *write* side's criterion, a distinct (usually larger) set from
    // UNION_INTERN_INFECTED's read side: because the generated Rust enum only
    // has one (latest) shape, write_foo_vN's match is exhaustive over the
    // *latest* variant set (falling back to DiscriminantOverflow for variants
    // whose discriminant doesn't fit this version's frozen width) — so it
    // needs the table if any variant it can actually still encode (latest,
    // width-filtered) uses intern, not just the ones this version originally
    // declared.
    static UNION_WRITE_INTERN_INFECTED: RefCell<HashSet<TypeId>> = RefCell::new(HashSet::new());

    // whether to emit Serialize/Deserialize derives/impls in generated code.
    // defaults to pojoc-build's own "serde" cargo feature (matching build.rs
    // consumers, who control it that way) but callers that generate code at
    // runtime for varying consumers — namely pojoc-cli — can override it per
    // call with set_emit_serde() instead of needing a separate compile of
    // pojoc-build per desired setting.
    static EMIT_SERDE: Cell<bool> = const { Cell::new(cfg!(feature = "serde")) };
}

pub fn set_infected(names: HashSet<String>) -> HashSet<String> {
    INFECTED.with(|c| c.replace(names))
}

pub fn is_infected(name: &str) -> bool {
    INFECTED.with(|c| c.borrow().contains(name))
}

pub fn set_constraint_infected(names: HashSet<String>) -> HashSet<String> {
    CONSTRAINT_INFECTED.with(|c| c.replace(names))
}

pub fn is_constraint_infected(name: &str) -> bool {
    CONSTRAINT_INFECTED.with(|c| c.borrow().contains(name))
}

pub fn set_intern_infected(names: HashSet<String>) -> HashSet<String> {
    INTERN_INFECTED.with(|c| c.replace(names))
}

pub fn is_intern_infected(name: &str) -> bool {
    INTERN_INFECTED.with(|c| c.borrow().contains(name))
}

pub fn set_union_intern_infected(ids: HashSet<TypeId>) -> HashSet<TypeId> {
    UNION_INTERN_INFECTED.with(|c| c.replace(ids))
}

pub fn is_union_intern_infected(id: &TypeId) -> bool {
    UNION_INTERN_INFECTED.with(|c| c.borrow().contains(id))
}

pub fn set_union_write_intern_infected(ids: HashSet<TypeId>) -> HashSet<TypeId> {
    UNION_WRITE_INTERN_INFECTED.with(|c| c.replace(ids))
}

pub fn is_union_write_intern_infected(id: &TypeId) -> bool {
    UNION_WRITE_INTERN_INFECTED.with(|c| c.borrow().contains(id))
}

/// Overrides whether generated code emits Serialize/Deserialize, returning
/// the previous value so callers can restore it afterward. Defaults to
/// pojoc-build's own "serde" cargo feature until set.
pub fn set_emit_serde(value: bool) -> bool {
    EMIT_SERDE.with(|c| c.replace(value))
}

pub fn emit_serde() -> bool {
    EMIT_SERDE.with(|c| c.get())
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
    // wire discriminant width in bytes (1/2/4), see ResolvedEnum::byte_width
    Enum(TypeId, u8),
    Bitset(TypeId, usize),
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
    // box<T> heap indirection - the only way to break a self-referential or
    // mutually-recursive struct cycle; the analyzer rejects any cycle that
    // doesn't cross a Boxed.
    Boxed(Box<ResolvedTypeRef>),
    // (min:, max:) validation constraint, enforced on encode and decode.
    // doesn't change the wire format, just adds a range/length/count check.
    Constrained {
        inner: Box<ResolvedTypeRef>,
        min: Option<f64>,
        max: Option<f64>,
    },
    // intern <type>: opt-in string dedup, always wraps a bare string scalar.
    // same Rust type as plain string, only the wire repr (table index vs
    // inline bytes) and codegen differ.
    Interned(Box<ResolvedTypeRef>),
    ImportedSchema {
        alias: String,
        root_name: String,
        version: i128,
    },
}

impl ResolvedTypeRef {
    /// the directly nested type refs, in declaration order (key before value for
    /// the map kinds). The single place the shape of the recursion is written
    /// down: predicates over a type tree describe what they contribute at one
    /// node and lean on `children`/[`any_node`]/[`for_each_node`] to walk,
    /// rather than each re-deriving the wrapper arms and silently treating a
    /// variant added later as a leaf.
    ///
    /// Leaf variants (`Scalar`, `Enum`, `Bitset`, `Union`, `FixedString`,
    /// `VFloat`, `ImportedSchema`) yield nothing — a named type's *fields* are
    /// not children here, since resolving those needs a registry. Walkers that
    /// cross into a named type do so explicitly.
    pub fn children(&self) -> impl Iterator<Item = &ResolvedTypeRef> {
        use ResolvedTypeRef::*;

        // a fixed-size array covers the one-inner and two-inner cases without
        // allocating; Tuple is the only variadic arm
        let (pair, list): ([Option<&ResolvedTypeRef>; 2], &[ResolvedTypeRef]) = match self {
            Array(inner)
            | FixedArray(inner, _)
            | DeltaArray(inner)
            | FixedDeltaArray(inner, _)
            | Optional(inner)
            | Boxed(inner)
            | Constrained { inner, .. }
            | Interned(inner) => ([Some(inner), None], &[]),

            Map(k, v) | FixedMap(k, v, _) => ([Some(k), Some(v)], &[]),

            Tuple(elems) => ([None, None], elems.as_slice()),

            Scalar(_)
            | Enum(..)
            | Bitset(..)
            | Union(_)
            | FixedString(_)
            | VFloat { .. }
            | ImportedSchema { .. } => ([None, None], &[]),
        };

        pair.into_iter().flatten().chain(list.iter())
    }

    /// true if `f` holds at this node or anywhere beneath it
    pub fn any_node(&self, f: &mut impl FnMut(&ResolvedTypeRef) -> bool) -> bool {
        f(self) || self.children().any(|c| c.any_node(f))
    }

    /// visits this node then every node beneath it, parents before children
    pub fn for_each_node(&self, f: &mut impl FnMut(&ResolvedTypeRef)) {
        f(self);
        for c in self.children() {
            c.for_each_node(f);
        }
    }

    pub fn type_id(&self) -> Option<&TypeId> {
        match self {
            ResolvedTypeRef::Scalar(id) => Option::from(id),
            ResolvedTypeRef::Enum(id, _) => Some(id),
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

mod primitive;
pub(crate) use primitive::primitive_info;

pub fn normalize_type(name: &str) -> &str {
    match name {
        "byte" | "uchar" => "u8",
        "ushort" => "u16",
        "uint" => "u32",
        "ulong" => "u64",
        "char" => "i8",
        "short" => "i16",
        "int" => "i32",
        "long" => "i64",
        "float" => "f32",
        "double" => "f64",
        "boolean" => "bool",
        "str" => "string",
        other => other,
    }
}

// Packed numeric arrays can bulk-cast all bit patterns. Bool cannot.
pub fn is_bulk_castable_scalar(name: &str) -> bool {
    let name = normalize_type(name);
    name != "bool"
        && primitive_info(name)
            .is_some_and(|primitive| matches!(primitive.wire_size, WireSize::Fixed(_)))
}

pub fn is_primitive(name: &str) -> bool {
    primitive_info(name).is_some()
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
                    let deref = if is_ref { "*" } else { "" };
                    format!("{f}({deref}{accessor} as usize)")
                } else {
                    let borrow_symbol = if is_ref { "" } else { "&" };
                    // strip <'buf> so an infected NestedLeaf<'buf> maps to size_hint_nested_leaf
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

// (rust integer type, read fn, write fn) for a 1/2/4-byte discriminant width.
pub(crate) fn width_prim_names(width: u8) -> (&'static str, &'static str, &'static str) {
    match width {
        1 => ("u8", "read_u8", "write_u8"),
        2 => ("u16", "read_u16", "write_u16"),
        _ => ("u32", "read_u32", "write_u32"),
    }
}

pub fn type_info(ty: &ResolvedTypeRef) -> TypeInfo {
    match ty {
        ResolvedTypeRef::Scalar(id) => scalar_info(&id.name),

        ResolvedTypeRef::Enum(id, width) => {
            let (_, read_prim, write_prim) = width_prim_names(*width);
            TypeInfo {
                wire_size: WireSize::Fixed(*width as usize),
                rust_type: id.name.clone(),
                skip_stmt: format!("let _ = {read_prim}(__buf, __pos)?;"),
                read_fn: read_prim.into(),
                write_fn: write_prim.into(),
                default_expr: format!("{}::default()", id.name),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Union(id) => {
            // one codec per resolved union version; the TypeId's version picks which.
            let lower = format!("{}_v{}", id.name.to_snake_case(), id.version);
            // helper fn names stay lifetime-free; the lifetime is inferred at the call site
            let rust_type = if is_infected(&id.name) {
                format!("{}<'buf>", id.name)
            } else {
                id.name.clone()
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

        ResolvedTypeRef::Bitset(id, width) => {
            let lower = format!("{}_v{}", id.name.to_snake_case(), id.version);
            TypeInfo {
                wire_size: WireSize::Fixed(*width),
                rust_type: id.name.clone(),
                skip_stmt: if *width == 0 {
                    String::new()
                } else {
                    format!(
                        "{{ let __end = __pos.checked_add({width}).ok_or(Error::UnexpectedEof)?; \
                             if __end > __buf.len() {{ return Err(Error::UnexpectedEof); }} \
                             *__pos = __end; }}"
                    )
                },
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
            let skip_stmt = if let ResolvedTypeRef::Scalar(id) = inner.as_ref()
                && is_bulk_castable_scalar(&id.name)
                && let WireSize::Fixed(width) = i.wire_size
            {
                let (to_elements, to_bytes) = if width > 1 {
                    (format!(" / {width}"), format!(" * {width}"))
                } else {
                    (String::new(), String::new())
                };
                // Advance by complete elements on EOF, just as the scalar reads do.
                format!(
                    "{{ let __n = read_array_len(__buf, __pos)?; \
                     let __available = __buf.len().saturating_sub(*__pos){to_elements}; \
                     *__pos += __n.min(__available){to_bytes}; \
                     if __n > __available {{ return Err(Error::UnexpectedEof); }} }}"
                )
            } else {
                format!(
                    "{{ let __n = read_array_len(__buf, __pos)?; for _ in 0..__n {{ {} }} }}",
                    i.skip_stmt
                )
            };
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: format!("PojocVec<{}>", i.rust_type),
                skip_stmt,
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
                default_expr: "PojocMap::default()".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::FixedMap(k, v, n) => {
            let (ki, vi) = (type_info(k), type_info(v));
            TypeInfo {
                wire_size: if let WireSize::Fixed(k) = ki.wire_size {
                    if let WireSize::Fixed(v) = vi.wire_size {
                        k.checked_add(v)
                            .and_then(|width| width.checked_mul(*n))
                            .map_or(WireSize::Variable, WireSize::Fixed)
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
                // a fixed map must always carry exactly N entries on the wire;
                // ::new() would give 0, so use the Default impl that pre-fills
                // N zero-valued (K::default(), V::default()) pairs instead
                default_expr: "Default::default()".into(),
                size_fn: None,
            }
        }

        ResolvedTypeRef::Tuple(elems) => {
            let infos: Vec<_> = elems.iter().map(type_info).collect();
            // a tuple is fixed-width only if every element is fixed-width and Copy.
            // FixedMap is fixed-size but not Copy, so a tuple containing one stays
            // Variable to avoid an invalid [default; N] init.
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
            // clippy's box_default lint wants `Box::default()` over
            // `Box::new(X::default())` — sound whenever the inner default is
            // itself a bare `X::default()` call, since Box<T> is Default for
            // any T: Default via std's blanket impl regardless of T's size.
            let default_expr = if i.default_expr.ends_with("::default()") {
                "Box::default()".to_string()
            } else {
                format!("Box::new({})", i.default_expr)
            };
            TypeInfo {
                // heap indirection doesn't change the wire bytes, only the in-memory field
                wire_size: i.wire_size,
                rust_type: format!("Box<{}>", i.rust_type),
                skip_stmt: i.skip_stmt.clone(),
                read_fn: "/* boxed expressions are handled inline */".into(),
                write_fn: "/* boxed expressions are handled inline */".into(),
                default_expr,
                size_fn: None,
            }
        }

        // constraints are transparent to type_info; codegen inspects the bounds
        // separately (compute_constraint_infected, encode.rs/decode.rs guards)
        ResolvedTypeRef::Constrained { inner, .. } => type_info(inner),

        ResolvedTypeRef::Interned(inner) => {
            let i = type_info(inner);
            TypeInfo {
                wire_size: WireSize::Variable,
                rust_type: i.rust_type,
                // skips just the index varint - the string bytes live in the table
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
            // an infected imported root is tracked in INFECTED under its module-qualified name
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
    let name = normalize_type(name);
    if let Some(primitive) = primitive_info(name) {
        let read_fn = format!("read_{name}");
        let skip_stmt = match primitive.wire_size {
            WireSize::Fixed(_) => format!("let _ = {read_fn}(__buf, __pos)?;"),
            WireSize::Variable => format!("skip_{name}(__buf, __pos)?;"),
        };
        return TypeInfo {
            wire_size: primitive.wire_size,
            rust_type: primitive.rust_type.into(),
            skip_stmt,
            read_fn,
            write_fn: format!("write_{name}"),
            default_expr: primitive.rust_default.into(),
            size_fn: matches!(name, "varint32" | "varint64").then(|| "varint_size".into()),
        };
    }

    let lower = name.to_snake_case();
    // Helper names stay lifetime-free. The call site infers the lifetime.
    let rust_type = if is_infected(name) {
        format!("{name}<'buf>")
    } else {
        name.into()
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

// FixedMap/Boxed are fixed-width but not Copy; used to keep them out of
// Copy-requiring fixed-array/fixed-block init paths.
pub fn contains_fixed_map(ty: &ResolvedTypeRef) -> bool {
    match ty {
        ResolvedTypeRef::FixedMap(..) => true,
        ResolvedTypeRef::Boxed(..) => true,
        ResolvedTypeRef::FixedArray(inner, _) => contains_fixed_map(inner),
        ResolvedTypeRef::Constrained { inner, .. } => contains_fixed_map(inner),
        ResolvedTypeRef::Tuple(elems) => elems.iter().any(contains_fixed_map),
        _ => false,
    }
}

// true if `ty` reaches an Enum through a FixedArray boundary — its unchecked
// read builds elements via a closure that can't propagate Enum's fallible `?`.
pub fn contains_enum_under_closure(ty: &ResolvedTypeRef) -> bool {
    fn go(ty: &ResolvedTypeRef, under_closure: bool) -> bool {
        match ty {
            ResolvedTypeRef::Enum(..) => under_closure,
            ResolvedTypeRef::FixedArray(inner, _) => go(inner, true),
            ResolvedTypeRef::Tuple(elems) => elems.iter().any(|e| go(e, under_closure)),
            ResolvedTypeRef::Constrained { inner, .. } => go(inner, under_closure),
            _ => false,
        }
    }
    go(ty, false)
}

pub fn contains_interned(ty: &ResolvedTypeRef) -> bool {
    ty.any_node(&mut |t| matches!(t, ResolvedTypeRef::Interned(_)))
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
