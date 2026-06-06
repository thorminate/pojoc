#[derive(Debug)]
pub struct SchemaAst {
    pub name: String,
    pub versions: Vec<VersionAst>,
}

#[derive(Debug)]
pub struct VersionAst {
    pub version: u32,
    pub blocks: Vec<VersionBlockAst>,
}

#[derive(Debug)]
pub enum VersionBlockAst {
    TypeDef(TypeDefAst),
    Fields(FieldsAst),
    Diff(Vec<DiffAst>),
}

#[derive(Debug)]
pub struct TypeDefAst {
    pub name: String,
    pub extends: Option<String>,
    pub body: TypeBody,
}

#[derive(Debug)]
pub enum TypeBody {
    Fields(Vec<FieldAst>),   // no extends — full definition
    Diff(Vec<DiffAst>),      // extends present — diff only
}

#[derive(Debug)]
pub struct FieldsAst {
    pub fields: Vec<FieldAst>,
}

#[derive(Debug)]
pub struct FieldAst {
    pub name: String,
    pub ty: TypeAst,
    pub default: Option<DefaultValueAst>,
}

#[derive(Debug, Clone)]
pub enum DefaultValueAst {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    EmptyArray,
}

#[derive(Debug)]
pub enum TypeAst {
    Named(String),
    Array(Box<TypeAst>),
    // add more later
}

#[derive(Debug)]
pub enum DiffAst {
    Add { field: FieldAst, },
    Remove { name: String },
    Rename { from: String, to: String },
    UpdateType { name: String, ty: TypeAst },
    Transform { from: String, to: String, ty: Option<TypeAst> }
}