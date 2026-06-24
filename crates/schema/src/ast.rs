use crate::span::Span;

#[derive(Debug)]
pub struct SchemaAst {
    pub name: String,
    pub imports: Vec<ImportDeclAst>,
    pub versions: Vec<VersionAst>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug)]
pub struct VersionAst {
    pub version: i128,
    pub blocks: Vec<VersionBlockAst>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug)]
pub enum VersionBlockAst {
    EnumDef(EnumDefAst),
    UnionDef(UnionDefAst),
    TypeDef(TypeDefAst),
    Fields(FieldsAst),
    Diff(Vec<DiffAst>),
    BitsetDef(BitsetDefAst),
}

#[derive(Debug, Clone)]
pub struct ExtendsAst {
    pub name: String,
    pub version: i128,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug)]
pub struct TypeDefAst {
    pub name: String,
    pub extends: Option<ExtendsAst>,
    pub body: TypeBody,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug)]
pub enum TypeBody {
    Fields(FieldsAst),
    Diff(Vec<DiffAst>),
}

#[derive(Debug, Clone)]
pub struct EnumVariantNode {
    pub name: String,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug)]
pub enum EnumDefAst {
    Definition {
        name: String,
        variants: Vec<EnumVariantNode>,
        span: Span,
        line: u32,
    },
    Extension {
        name: String,
        base: ExtendsAst,
        ops: Vec<EnumVariantOpAst>,
        span: Span,
        line: u32,
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

#[derive(Debug, Clone)]
pub enum EnumVariantOpAst {
    Add {
        name: String,
        span: Span,
        line: u32,
    },
    Rename {
        from: String,
        to: String,
        span: Span,
        line: u32,
    },
}

#[derive(Debug)]
pub enum BitsetDefAst {
    Definition {
        name: String,
        variants: Vec<String>,
        span: Span,
        line: u32,
    },
    Extension {
        name: String,
        base: ExtendsAst,
        ops: Vec<BitsetOpAst>,
        span: Span,
        line: u32,
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

#[derive(Debug)]
pub enum BitsetOpAst {
    Add { name: String, span: Span, line: u32 },
    Remove { name: String, span: Span, line: u32 },
}

#[derive(Debug, Clone)]
pub struct UnionVariantAst {
    pub name: String,
    pub payload_ty: TypeAst,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum UnionDefAst {
    Definition {
        name: String,
        variants: Vec<UnionVariantAst>,
        span: Span,
        line: u32,
    },
    Extension {
        name: String,
        base: ExtendsAst,
        ops: Vec<UnionVariantOpAst>,
        span: Span,
        line: u32,
    },
}

impl UnionDefAst {
    pub fn name(&self) -> &str {
        match self {
            UnionDefAst::Definition { name, .. } => name,
            UnionDefAst::Extension { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone)]
pub enum UnionVariantOpAst {
    Add {
        name: String,
        payload_ty: TypeAst,
        span: Span,
        line: u32,
    },
}

#[derive(Debug, Clone)]
pub struct FieldsAst {
    pub fields: Vec<FieldAst>,
    pub const_fields: Vec<ConstFieldAst>,
}

#[derive(Debug, Clone)]
pub struct FieldAst {
    pub name: String,
    pub ty: TypeAst,
    pub default: Option<DefaultValueAst>,
    pub lazy: bool,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct ConstFieldAst {
    pub name: String,
    pub ty: TypeAst,
    pub value: DefaultValueAst,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct ImportDeclAst {
    pub path: String,
    pub alias: String,
    pub span: Span,
    pub line: u32,
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
    Repeat(Box<DefaultValueAst>),
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
    Imported { alias: String, version: i128 },
}

#[derive(Debug, Clone)]
pub enum DiffAst {
    Add {
        field: FieldAst,
    },
    AddConst {
        field: ConstFieldAst,
    },
    Remove {
        name: String,
        span: Span,
        line: u32,
    },
    Rename {
        from: String,
        to: String,
        span: Span,
        line: u32,
    },
    UpdateType {
        name: String,
        ty: TypeAst,
        lazy: bool,
        span: Span,
        line: u32,
    },
    Transform {
        from: String,
        to: String,
        ty: Option<TypeAst>,
        lazy: bool,
        span: Span,
        line: u32,
    },
    UpdateConst {
        name: String,
        ty: TypeAst,
        value: DefaultValueAst,
        span: Span,
        line: u32,
    },
    TransformConst {
        from: String,
        to: String,
        ty: TypeAst,
        value: DefaultValueAst,
        span: Span,
        line: u32,
    },
}
