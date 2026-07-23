use crate::schema::span::Span;

#[derive(Debug)]
pub struct SchemaAst {
    pub name: String,
    pub imports: Vec<ImportDeclAst>,
    pub versions: Vec<VersionAst>,
    // doc comments above the `schema` header, emitted on the generated root struct
    pub doc: Vec<String>,
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
pub enum GenericArgAst {
    Type(TypeAst),
    Wildcard,
}

#[derive(Debug, Clone)]
pub struct ExtendsAst {
    pub name: String,
    pub version: i128,
    pub args: Vec<GenericArgAst>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct TypeDefAst {
    pub name: String,
    pub params: Vec<String>,
    pub extends: Option<ExtendsAst>,
    pub body: TypeBody,
    // doc comments above the `type` header
    pub doc: Vec<String>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum TypeBody {
    Fields(FieldsAst),
    Diff(Vec<DiffAst>),
}

#[derive(Debug, Clone)]
pub struct EnumVariantNode {
    pub name: String,
    pub doc: Vec<String>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug)]
pub enum EnumDefAst {
    Definition {
        name: String,
        variants: Vec<EnumVariantNode>,
        doc: Vec<String>,
        span: Span,
        line: u32,
    },
    Extension {
        name: String,
        base: ExtendsAst,
        ops: Vec<EnumVariantOpAst>,
        // overrides the base version's doc for this and later versions if non-empty
        doc: Vec<String>,
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
        doc: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct BitsetVariantNode {
    pub name: String,
    pub doc: Vec<String>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug)]
pub enum BitsetDefAst {
    Definition {
        name: String,
        variants: Vec<BitsetVariantNode>,
        doc: Vec<String>,
        span: Span,
        line: u32,
    },
    Extension {
        name: String,
        base: ExtendsAst,
        ops: Vec<BitsetOpAst>,
        /// Overrides the bitset's own doc for this and later versions when
        /// non-empty; otherwise it keeps whatever doc the base version had.
        doc: Vec<String>,
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
    Add {
        name: String,
        doc: Vec<String>,
        span: Span,
        line: u32,
    },
    Remove {
        name: String,
        span: Span,
        line: u32,
    },
}

#[derive(Debug, Clone)]
pub struct UnionVariantAst {
    pub name: String,
    pub payload_ty: TypeAst,
    pub doc: Vec<String>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum UnionDefAst {
    Definition {
        name: String,
        variants: Vec<UnionVariantAst>,
        doc: Vec<String>,
        span: Span,
        line: u32,
    },
    Extension {
        name: String,
        base: ExtendsAst,
        ops: Vec<UnionVariantOpAst>,
        /// Overrides the union's own doc for this and later versions when
        /// non-empty; otherwise it keeps whatever doc the base version had.
        doc: Vec<String>,
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
        doc: Vec<String>,
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
    pub doc: Vec<String>,
    pub span: Span,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct ConstFieldAst {
    pub name: String,
    pub ty: TypeAst,
    pub value: DefaultValueAst,
    pub doc: Vec<String>,
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
    /// `Name<args>`, with an optional `as Alias` naming the monomorphized
    /// instantiation instead of using the auto-mangled name.
    Generic(String, Vec<TypeAst>, Option<String>),
    Optional(Box<TypeAst>),
    Array(Box<TypeAst>),
    FixedArray(Box<TypeAst>, usize),
    DeltaArray(Box<TypeAst>),
    FixedDeltaArray(Box<TypeAst>, usize),
    FixedString(usize),
    Map(Box<TypeAst>, Box<TypeAst>),
    FixedMap(Box<TypeAst>, Box<TypeAst>, usize),
    Tuple(Vec<TypeAst>),
    VFloat {
        min: f64,
        max: f64,
        step: f64,
    },
    /// A postfix `(min: .., max: ..)` validation constraint on a number,
    /// array, map, or string type — distinct from `VFloat`'s own `min`/`max`,
    /// which control quantization, not range validation.
    Constrained {
        inner: Box<TypeAst>,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// `intern <type>` — a type-level wrapper (not just a field-position
    /// keyword), so it composes inside containers/generics: `[intern
    /// string]`, `map<K, intern string>`, `Mono<intern string>`. Parsed as a
    /// prefix wherever `parse_type` is invoked, including recursively.
    /// Semantically only valid wrapping a bare `string` — enforced at
    /// analysis time once the inner type is resolved, not here.
    Interned(Box<TypeAst>),
    Imported {
        alias: String,
        version: i128,
    },
    /// Internal-only marker produced when a generic ancestor's type parameter is
    /// dropped via `_` in an `extends<...>` argument list. Never produced by the
    /// parser; must not survive past `template_shape` resolution.
    Wildcard,
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
