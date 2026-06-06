use std::collections::HashMap;
use crate::ast::DefaultValueAst;
use super::lineage::SchemaLineage;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeId {
    pub name: String,
    pub version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldId(pub u64);

#[derive(Debug, Default)]
pub struct TypeRegistry {
    pub types: HashMap<TypeId, ResolvedType>,
}

impl TypeRegistry {
    pub fn latest_before(&self, name: &str, before_version: u32) -> Option<&ResolvedType> {
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

#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedTypeRef {
    Scalar(TypeId),
    Array(Box<ResolvedTypeRef>),
}

impl ResolvedTypeRef {
    pub fn type_id(&self) -> &TypeId {
        match self {
            ResolvedTypeRef::Scalar(id) => id,
            ResolvedTypeRef::Array(id) => id.type_id(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FieldIR {
    pub id: FieldId,
    pub name: String,
    pub ty: ResolvedTypeRef,
    pub default: Option<DefaultValue>,
}

#[derive(Clone, Debug)]
pub struct VersionContext {
    pub version: u32,
    pub fields: Vec<FieldIR>,
}

#[derive(Debug)]
pub struct ResolvedType {
    pub fields: Vec<FieldIR>,
}

#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub version: u32,
    pub fields: Vec<FieldIR>,
}

#[derive(Debug)]
pub struct ResolvedSchema {
    pub name_hint: String,
    pub versions: Vec<ResolvedVersion>,
    pub types: TypeRegistry,
    pub lineage: SchemaLineage,
}

#[derive(Debug, Clone)]
pub enum DefaultValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    EmptyArray,
    Struct,
}

impl From<&DefaultValueAst> for DefaultValue {
    fn from(ast: &DefaultValueAst) -> Self {
        match ast {
            DefaultValueAst::Bool(b)  => DefaultValue::Bool(*b),
            DefaultValueAst::Int(i)   => DefaultValue::Int(*i),
            DefaultValueAst::Float(f) => DefaultValue::Float(*f),
            DefaultValueAst::Str(s)   => DefaultValue::Str(s.clone()),
            DefaultValueAst::EmptyArray => DefaultValue::EmptyArray,
        }
    }
}