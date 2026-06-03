use pojoc_core::Type;

pub struct SchemaAst {
    pub name: String,
    pub versions: Vec<VersionAst>,
}

pub struct VersionAst {
    pub number: u32,
    pub blocks: Vec<VersionBlockAst>,
}

pub enum VersionBlockAst {
    TypeDef(TypeDefAst),
    Fields(FieldsAst),
    Diff(Vec<DiffAst>),
}

pub struct TypeDefAst {
    pub name: String,
    pub fields: Vec<FieldAst>,
}

pub struct FieldsAst {
    pub fields: Vec<FieldAst>,
}

pub struct FieldAst {
    pub name: String,
    pub ty: Type,
}

pub enum DiffAst {
    Add { field: FieldAst },

    Remove { name: String },

    Rename { from: String, to: String },

    UpdateType { name: String, ty: Type },
}