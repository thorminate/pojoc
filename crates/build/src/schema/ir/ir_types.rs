use super::lineage::SchemaLineage;
use crate::core::types::*;
use crate::schema::ast::DefaultValueAst;
use crate::schema::error::SchemaWarning;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldId(pub u64);

#[derive(Debug, Default)]
pub struct TypeRegistry {
    pub types: HashMap<TypeId, ResolvedType>,
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
    /// only ever set on root schema fields, see SeekableOnlyOnRootFields
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
    pub fn byte_width(&self) -> u8 {
        match self.variants.len() {
            n if n <= 8 => 1,
            n if n <= 16 => 2,
            _ => 4,
        }
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
    pub(crate) import_paths: HashMap<String, PathBuf>,
    /// doc comments above the `schema` header, emitted on the generated root struct
    pub doc: Vec<String>,
    /// non-fatal diagnostics for the caller (CLI, build script, LSP) to surface
    pub warnings: Vec<SchemaWarning>,
}

impl ResolvedSchema {
    pub fn warnings_recursive<'a>(&'a self, path: &Path) -> Vec<(&'a SchemaWarning, PathBuf)> {
        fn collect<'a>(
            schema: &'a ResolvedSchema,
            path: PathBuf,
            visited: &mut HashSet<*const ResolvedSchema>,
            out: &mut Vec<(&'a SchemaWarning, PathBuf)>,
        ) {
            if !visited.insert(schema) {
                return;
            }
            out.extend(
                schema
                    .warnings
                    .iter()
                    .map(|warning| (warning, path.clone())),
            );
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            let mut imports: Vec<_> = schema.import_paths.iter().collect();
            imports.sort_unstable_by_key(|(alias, _)| *alias);
            for (alias, relative) in imports {
                if let Some(imported) = schema.imports.get(alias) {
                    collect(imported, parent.join(relative), visited, out);
                }
            }
        }

        let mut warnings = Vec::new();
        collect(self, path.into(), &mut HashSet::new(), &mut warnings);
        warnings
    }
}

pub fn latest_by_name<T>(items: &HashMap<TypeId, T>) -> HashMap<String, (i128, &T)> {
    let mut latest = HashMap::new();
    for (id, value) in items {
        let entry = latest.entry(id.name.clone()).or_insert((id.version, value));
        if id.version > entry.0 {
            *entry = (id.version, value);
        }
    }
    latest
}

pub(crate) fn width_max_value(width: u8) -> u64 {
    (1u64 << (width * 8)) - 1
}

pub(crate) fn type_reaches_intern_table(
    types: &TypeRegistry,
    unions: &UnionRegistry,
    ty: &ResolvedTypeRef,
    visited: &mut HashSet<TypeId>,
) -> bool {
    ty.any_node(&mut |part| match part {
        ResolvedTypeRef::Interned(_) => true,
        ResolvedTypeRef::Scalar(id) if !is_primitive(&id.name) && visited.insert(id.clone()) => {
            types.types.get(id).is_some_and(|resolved| {
                resolved
                    .fields
                    .iter()
                    .any(|f| type_reaches_intern_table(types, unions, &f.ty, visited))
            })
        }
        ResolvedTypeRef::Union(id) if visited.insert(id.clone()) => {
            let Some(pinned) = unions.unions.get(id) else {
                return false;
            };
            let max = width_max_value(pinned.discriminant_width());
            unions
                .unions
                .iter()
                .filter(|(candidate, _)| candidate.name == id.name)
                .max_by_key(|(candidate, _)| candidate.version)
                .is_some_and(|(_, latest)| {
                    latest.variants.iter().any(|variant| {
                        variant.discriminant <= max
                            && type_reaches_intern_table(types, unions, &variant.payload, visited)
                    })
                })
        }
        // An imported schema starts its own intern table.
        _ => false,
    })
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
