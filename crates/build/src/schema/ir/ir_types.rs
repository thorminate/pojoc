use super::lineage::SchemaLineage;
use crate::core::types::*;
use crate::schema::ast::DefaultValueAst;
use crate::schema::error::SchemaWarning;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldId(pub u64);

#[derive(Debug, Default)]
pub struct TypeRegistry {
    pub types: HashMap<TypeId, ResolvedType>,
}

/// highest-versioned entry per type name. `type Foo extends Foo@N` mints a new
/// TypeId per version rather than mutating the old one, so "the current shape of
/// Foo" is always a max-by-version lookup.
pub fn latest_by_name<V>(items: &HashMap<TypeId, V>) -> HashMap<String, (i128, &V)> {
    let mut latest: HashMap<String, (i128, &V)> = HashMap::new();
    for (id, value) in items {
        let entry = latest.entry(id.name.clone()).or_insert((id.version, value));
        if id.version > entry.0 {
            *entry = (id.version, value);
        }
    }
    latest
}

/// widest discriminant value a union of this byte width can carry
pub fn width_max_value(width: u8) -> u64 {
    (1u128 << (width as u32 * 8)) as u64 - 1
}

/// whether a value of this type reaches the message's shared intern table,
/// following named struct and union types into their members.
///
/// Deliberately the *write-broadened* criterion: the Union arm walks the union's
/// latest variant set (filtered by the pinned version's discriminant width), not
/// just the pinned version's own variants. `write_foo_vN`'s match is exhaustive
/// over the one Rust enum shape, so an old-pinned encoder can be handed a
/// variant only a later version declared. Anything deciding "does the table need
/// to exist here" must agree with that possibility.
///
/// Shared by the analyzer (rejecting `lazy` + interned, which would defer to a
/// decode helper that has nowhere to receive the table) and codegen (threading
/// `__table`, and picking a seek's intern strategy). One implementation so the
/// two cannot drift.
pub fn type_reaches_intern_table(
    types: &TypeRegistry,
    unions: &UnionRegistry,
    ty: &ResolvedTypeRef,
    visited: &mut std::collections::HashSet<TypeId>,
) -> bool {
    let recurse = |t: &ResolvedTypeRef, v: &mut std::collections::HashSet<TypeId>| {
        type_reaches_intern_table(types, unions, t, v)
    };

    // the wrapper/container arms are just the generic descent; only the two
    // registry-crossing leaves need their own reasoning
    let here = match ty {
        ResolvedTypeRef::Interned(_) => true,

        // TypeId carries the version, so recurse into that exact historical
        // shape, not the latest one
        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) => {
            if !visited.insert(id.clone()) {
                return false;
            }
            types
                .types
                .get(id)
                .is_some_and(|r| r.fields.iter().any(|f| recurse(&f.ty, visited)))
        }

        // see the write-broadened note above: NOT the same reasoning as Scalar
        ResolvedTypeRef::Union(id) => {
            if !visited.insert(id.clone()) {
                return false;
            }
            let Some(pinned) = unions.unions.get(id) else {
                return false;
            };
            let latest = latest_by_name(&unions.unions);
            let Some((_, latest)) = latest.get(&id.name) else {
                return false;
            };
            let max_value = width_max_value(pinned.discriminant_width());
            latest
                .variants
                .iter()
                .filter(|v| v.discriminant <= max_value)
                .any(|v| recurse(&v.payload, visited))
        }

        // an imported root writes its own intern table inline into its own
        // nested body, so it never reaches the enclosing message's table
        _ => false,
    };

    here || ty.children().any(|c| recurse(c, visited))
}

impl TypeRegistry {
    pub fn latest_before(&self, name: &str, before_version: i128) -> Option<&ResolvedType> {
        self.types
            .iter()
            .filter(|(id, _)| id.name == name && id.version < before_version)
            .max_by_key(|(id, _)| id.version)
            .map(|(_, ty)| ty)
    }
}

#[derive(Debug, Clone)]
pub enum TypeIR {
    U32,
    I32,
    F32,
    String,
    Array(Box<TypeIR>),
    Struct(TypeId),
}

#[derive(Debug, Clone)]
pub struct FieldIR {
    pub id: FieldId,
    pub name: String,
    pub ty: ResolvedTypeRef,
    pub default: Option<DefaultValue>,
    pub lazy: bool,
    /// enables a root accessor or a hop in a nested pluck path
    pub seekable: bool,
    /// doc lines carried through unchanged across renames/retypes, like default
    pub doc: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct VersionContext {
    pub version: i128,
    pub fields: Vec<FieldIR>,
    pub const_fields: Vec<ResolvedConst>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub wire_value: u32,
    pub doc: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedEnum {
    pub variants: Vec<EnumVariant>,
    pub doc: Vec<String>,
}

impl ResolvedEnum {
    pub fn wire_value(&self, variant_name: &str) -> Option<u32> {
        self.variants
            .iter()
            .find(|v| v.name == variant_name)
            .map(|v| v.wire_value)
    }

    pub fn next_wire_value(&self) -> u32 {
        self.variants
            .iter()
            .map(|v| v.wire_value)
            .max()
            .map_or(0, |m| m + 1)
    }

    /// smallest unsigned width (1/2/4 bytes) that can hold every wire value
    /// this enum declares, including the compiler-injected `Unknown = 0`.
    pub fn byte_width(&self) -> u8 {
        discriminant_byte_width(self.variants.len())
    }
}

#[derive(Debug, Default)]
pub struct EnumRegistry {
    pub enums: HashMap<TypeId, ResolvedEnum>,
}

impl EnumRegistry {
    pub fn latest_before(
        &self,
        name: &str,
        before_version: i128,
    ) -> Option<(&TypeId, &ResolvedEnum)> {
        self.enums
            .iter()
            .filter(|(id, _)| id.name == name && id.version < before_version)
            .max_by_key(|(id, _)| id.version)
    }
}

#[derive(Debug, Clone)]
pub struct BitsetVariant {
    pub name: String,
    pub doc: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedBitset {
    pub variants: Vec<BitsetVariant>,
    pub doc: Vec<String>,
}

impl ResolvedBitset {
    pub fn byte_width(&self) -> usize {
        self.variants.len().div_ceil(8)
    }
    pub fn backing_type(&self) -> &'static str {
        match self.byte_width() {
            1 => "u8",
            2 => "u16",
            _ => "u32",
        }
    }
}

#[derive(Debug, Default)]
pub struct BitsetRegistry {
    pub bitsets: HashMap<TypeId, ResolvedBitset>,
}

#[derive(Debug, Clone)]
pub struct UnionVariant {
    pub name: String,
    pub payload: ResolvedTypeRef,
    pub discriminant: u64,
    pub doc: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedUnion {
    pub variants: Vec<UnionVariant>,
    pub doc: Vec<String>,
}

impl ResolvedUnion {
    /// smallest unsigned width (1/2/4 bytes) that fits every variant this
    /// specific resolved version declares, frozen per version rather than
    /// recomputed from the union's latest shape. See docs/schema.md's `union` section.
    pub fn discriminant_width(&self) -> u8 {
        discriminant_byte_width(self.variants.len())
    }
}

/// smallest unsigned width (1/2/4 bytes) that can represent `count` distinct
/// contiguous 0-based discriminant values — shared by `ResolvedEnum::byte_width`
/// and `ResolvedUnion::discriminant_width`.
fn discriminant_byte_width(count: usize) -> u8 {
    match count {
        n if n <= 256 => 1,
        n if n <= 65536 => 2,
        _ => 4,
    }
}

#[derive(Debug, Default)]
pub struct UnionRegistry {
    pub unions: HashMap<TypeId, ResolvedUnion>,
}

impl UnionRegistry {
    pub fn latest_before(
        &self,
        name: &str,
        before_version: i128,
    ) -> Option<(&TypeId, &ResolvedUnion)> {
        self.unions
            .iter()
            .filter(|(id, _)| id.name == name && id.version < before_version)
            .max_by_key(|(id, _)| id.version)
    }
}

#[derive(Debug)]
pub struct ResolvedType {
    pub fields: Vec<FieldIR>,
    pub const_fields: Vec<ResolvedConst>,
    pub doc: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedConst {
    pub name: String,
    pub rust_type: &'static str,
    pub value: DefaultValue,
    pub doc: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub version: i128,
    pub fields: Vec<FieldIR>,
    pub const_fields: Vec<ResolvedConst>,
}

#[derive(Debug)]
pub struct ResolvedSchema {
    pub name_hint: String,
    pub versions: Vec<ResolvedVersion>,
    pub types: TypeRegistry,
    pub enums: EnumRegistry,
    pub unions: UnionRegistry,
    pub bitsets: BitsetRegistry,
    pub lineage: SchemaLineage,
    pub imports: HashMap<String, Arc<ResolvedSchema>>,
    /// doc comments above the `schema` header, emitted on the generated root struct
    pub doc: Vec<String>,
    /// non-fatal diagnostics for the caller (CLI, build script, LSP) to surface
    pub warnings: Vec<SchemaWarning>,
    /// the file this schema was loaded from, when it came through the import
    /// orchestrator. Needed so an imported schema's warnings still render
    /// against their own source rather than the root's.
    pub source_path: Option<std::path::PathBuf>,
}

impl ResolvedSchema {
    /// this schema's warnings plus every imported schema's, each paired with the
    /// file it should be rendered against. Imports form a DAG (diamonds share an
    /// Arc), so each file is visited once.
    pub fn warnings_recursive<'a>(
        &'a self,
        root: &'a std::path::Path,
    ) -> Vec<(&'a SchemaWarning, &'a std::path::Path)> {
        let mut out = Vec::new();
        let mut seen: std::collections::HashSet<*const ResolvedSchema> =
            std::collections::HashSet::new();
        self.collect_warnings(root, &mut seen, &mut out);
        out
    }

    fn collect_warnings<'a>(
        &'a self,
        path: &'a std::path::Path,
        seen: &mut std::collections::HashSet<*const ResolvedSchema>,
        out: &mut Vec<(&'a SchemaWarning, &'a std::path::Path)>,
    ) {
        if !seen.insert(self as *const ResolvedSchema) {
            return;
        }
        out.extend(self.warnings.iter().map(|w| (w, path)));
        for imported in self.imports.values() {
            let own = imported.source_path.as_deref().unwrap_or(path);
            imported.collect_warnings(own, seen, out);
        }
    }
}

#[derive(Debug, Clone)]
pub enum DefaultValue {
    None,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Array(Vec<DefaultValue>),
    Map(Vec<(DefaultValue, DefaultValue)>),
    Struct,
    FixedBytes(Vec<u8>),
    Tuple(Vec<DefaultValue>),
    EnumVariant {
        ty_name: String,
        variant: String,
    },
    BitsetLiteral {
        ty_name: String,
        kvs: Vec<(String, bool)>,
    },
    Repeat(Box<DefaultValue>),
}

impl From<&DefaultValueAst> for DefaultValue {
    fn from(ast: &DefaultValueAst) -> Self {
        match ast {
            DefaultValueAst::Bool(b) => DefaultValue::Bool(*b),
            DefaultValueAst::Int(i) => DefaultValue::Int(*i),
            DefaultValueAst::Float(f) => DefaultValue::Float(*f),
            DefaultValueAst::Str(s) => DefaultValue::Str(s.clone()),
            DefaultValueAst::Array(els) => {
                DefaultValue::Array(els.iter().map(DefaultValue::from).collect())
            }
            DefaultValueAst::Map(pairs) => DefaultValue::Map(
                pairs
                    .iter()
                    .map(|(k, v)| (DefaultValue::from(k), DefaultValue::from(v)))
                    .collect(),
            ),
            DefaultValueAst::FixedBytes(b) => DefaultValue::FixedBytes(b.clone()),
            DefaultValueAst::Tuple(elements) => {
                DefaultValue::Tuple(elements.iter().map(DefaultValue::from).collect())
            }
            DefaultValueAst::EnumVariant { ty, variant } => DefaultValue::EnumVariant {
                ty_name: ty.clone(),
                variant: variant.clone(),
            },
            DefaultValueAst::BitsetLiteral { ty, kvs } => DefaultValue::BitsetLiteral {
                ty_name: ty.clone(),
                kvs: kvs.clone(),
            },
            DefaultValueAst::Repeat(inner) => {
                DefaultValue::Repeat(Box::new(DefaultValue::from(inner.as_ref())))
            }
        }
    }
}
