use super::lineage::SchemaLineage;
use crate::ast::DefaultValueAst;
use pojoc_core::types::*;
use std::collections::HashMap;
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
    pub lazy: bool
}

#[derive(Clone, Debug)]
pub struct VersionContext {
    pub version: i128,
    pub fields: Vec<FieldIR>,
    pub const_fields: Vec<ResolvedConst>
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub wire_value: u32,
}

#[derive(Debug, Clone)]
pub struct ResolvedEnum {
    pub variants: Vec<EnumVariant>,
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
            .map(|(id, e)| (id, e))
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedBitset {
    pub variants: Vec<String>,
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
    pub payload: TypeId,
    pub discriminant: u64,
}

#[derive(Debug, Clone)]
pub struct ResolvedUnion {
    pub variants: Vec<UnionVariant>,
}

#[derive(Debug, Default)]
pub struct UnionRegistry {
    pub unions: HashMap<TypeId, ResolvedUnion>,
}

impl UnionRegistry {
    pub fn latest_before(&self, name: &str, before_version: i128) -> Option<(&TypeId, &ResolvedUnion)> {
        self.unions
            .iter()
            .filter(|(id, _)| id.name == name && id.version < before_version)
            .max_by_key(|(id, _)| id.version)
            .map(|(id, u)| (id, u))
    }
}

#[derive(Debug)]
pub struct ResolvedType {
    pub fields: Vec<FieldIR>,
    pub const_fields: Vec<ResolvedConst>
}

#[derive(Debug, Clone)]
pub struct ResolvedConst {
    pub name: String,
    pub rust_type: &'static str,
    pub value: DefaultValue,
}

#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub version: i128,
    pub fields: Vec<FieldIR>,
    pub const_fields: Vec<ResolvedConst>
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
        }
    }
}
