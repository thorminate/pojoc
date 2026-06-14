#[derive(Debug)]
pub struct SchemaAst {
    pub name: String,
    pub versions: Vec<VersionAst>,
}

#[derive(Debug)]
pub struct VersionAst {
    pub version: i128,
    pub blocks: Vec<VersionBlockAst>,
}

#[derive(Debug)]
pub enum VersionBlockAst {
    EnumDef(EnumDefAst),
    TypeDef(TypeDefAst),
    Fields(FieldsAst),
    Diff(Vec<DiffAst>),
    BitsetDef(BitsetDefAst),
}

#[derive(Debug)]
pub struct ExtendsAst {
    pub name: String,
    pub version: i128,
}

#[derive(Debug)]
pub struct TypeDefAst {
    pub name: String,
    pub extends: Option<ExtendsAst>,
    pub body: TypeBody,
}

#[derive(Debug)]
pub enum TypeBody {
    Fields(FieldsAst),
    Diff(Vec<DiffAst>),
}

#[derive(Debug)]
pub enum EnumVariantOpAst {
    Add(String),
    Rename { from: String, to: String },
}

#[derive(Debug)]
pub enum EnumDefAst {
    Definition {
        name: String,
        variants: Vec<String>,
    },
    Extension {
        name: String,
        base: ExtendsAst, // reuse existing ExtendsAst
        ops: Vec<EnumVariantOpAst>,
    },
}

impl EnumDefAst {
    pub fn name(&self) -> &str {
        match self {
            EnumDefAst::Definition { name, .. } => name,
            EnumDefAst::Extension { name, .. } => name,
        }
    }
}

#[derive(Debug)]
pub enum BitsetOpAst {
    Add(String),
    Remove(String),
}

#[derive(Debug)]
pub enum BitsetDefAst {
    Definition {
        name: String,
        variants: Vec<String>,
    },
    Extension {
        name: String,
        base: ExtendsAst,
        ops: Vec<BitsetOpAst>,
    },
}

impl BitsetDefAst {
    pub fn name(&self) -> &str {
        match self {
            BitsetDefAst::Definition { name, .. } => name,
            BitsetDefAst::Extension { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstFieldAst {
    pub name: String,
    pub ty: TypeAst,
    pub value: DefaultValueAst,
}

#[derive(Debug)]
pub struct FieldsAst {
    pub fields: Vec<FieldAst>,
    pub const_fields: Vec<ConstFieldAst>,
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
    Int(i128),
    Float(f64),
    Str(String),
    Array(Vec<DefaultValueAst>),
    Map(Vec<(DefaultValueAst, DefaultValueAst)>),
    FixedBytes(Vec<u8>),
    Tuple(Vec<DefaultValueAst>),
    EnumVariant {
        ty: String,
        variant: String,
    },
    BitsetLiteral {
        ty: String,
        kvs: Vec<(String, bool)>,
    },
}

#[derive(Debug, Clone)]
pub enum TypeAst {
    Named(String),
    Optional(Box<TypeAst>),
    Array(Box<TypeAst>),
    FixedArray(Box<TypeAst>, usize),
    DeltaArray(Box<TypeAst>),
    FixedDeltaArray(Box<TypeAst>, usize),
    FixedString(usize),
    Map(Box<TypeAst>, Box<TypeAst>),
    FixedMap(Box<TypeAst>, Box<TypeAst>, usize),
    Tuple(Vec<TypeAst>),
    VFloat { min: f64, max: f64, step: f64 },
}

#[derive(Debug)]
pub enum DiffAst {
    Add {
        field: FieldAst,
    },
    AddConst {
        field: ConstFieldAst,
    },
    Remove {
        name: String,
    },
    Rename {
        from: String,
        to: String,
    },
    UpdateType {
        name: String,
        ty: TypeAst,
    },
    Transform {
        from: String,
        to: String,
        ty: Option<TypeAst>,
    },
}
