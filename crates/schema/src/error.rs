use crate::lexer::Token;
use crate::span::Span;
use thiserror::Error;

pub trait IndexableError {
    fn span(&self) -> Span;
    fn line(&self) -> u32;
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unexpected token `{got}`, expected {expected}")]
    UnexpectedToken {
        got: Token,
        expected: &'static str,
        span: Span,
        line: u32,
    },
    #[error("unexpected EOF")]
    UnexpectedEof,
    #[error("invalid syntax: `{message}`")]
    InvalidSyntax {
        message: String,
        span: Span,
        line: u32,
    },
}

impl IndexableError for ParseError {
    fn span(&self) -> Span {
        match self {
            ParseError::UnexpectedToken { span, .. } => *span,
            ParseError::UnexpectedEof => Span::new(0, 0),
            ParseError::InvalidSyntax { span, .. } => *span,
        }
    }

    fn line(&self) -> u32 {
        match self {
            ParseError::UnexpectedToken { line, .. } => *line,
            ParseError::UnexpectedEof => 0,
            ParseError::InvalidSyntax { line, .. } => *line,
        }
    }
}

#[derive(Debug, Error)]
pub enum LexError {
    #[error("unexpected character `{ch}`")]
    UnexpectedChar { ch: char, span: Span, line: u32 },
}

impl IndexableError for LexError {
    fn span(&self) -> Span {
        match self {
            LexError::UnexpectedChar { span, .. } => *span,
        }
    }

    fn line(&self) -> u32 {
        match self {
            LexError::UnexpectedChar { line, .. } => *line,
        }
    }
}

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("unknown type '{name}' referenced in version {version}")]
    UnknownType {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("type '{child}' in version {version} extends unknown type '{parent}'")]
    UnknownParentType {
        child: String,
        parent: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: enum '{type_name}' has no variant '{variant}'")]
    UnknownEnumVariant {
        type_name: String,
        variant: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("type '{name}' in version {version} uses 'extends' but body must be a diff")]
    ExtendsWithFullDefinition {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "diff op '{op}' references unknown field '{field}' in type '{type_name}' version {version}"
    )]
    FieldNotFound {
        op: &'static str,
        field: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("field '{field}' in version {version} doesn't have a default value")]
    MissingDefault {
        field: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version} has a fixed {kind} of {expected} entries but default value has {got} entries"
    )]
    FixedSizeDefaultLengthMismatch {
        field: String,
        kind: &'static str,
        expected: usize,
        got: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: diff adds field `{field}` but it already exists")]
    FieldAlreadyExists {
        version: i128,
        field: String,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version} has a fixed string of {expected} bytes but default value is {got} bytes"
    )]
    FixedStringDefaultLengthMismatch {
        field: String,
        expected: usize,
        got: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: {kind} fixed size {n} is too large, consider not using a fixed size"
    )]
    FixedSizeTooLarge {
        kind: &'static str,
        n: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        expected: String,
        got: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: varints cannot be used in const fields")]
    VarintsCannotBeConst {
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: invalid vfloat: {reason}")]
    InvalidVFloat {
        reason: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: vfloat range is too large: {range}")]
    VFloatRangeTooLarge {
        range: f64,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max})"
    )]
    VFloatDefaultOutOfRange {
        field: String,
        value: f64,
        min: f64,
        max: f64,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max})"
    )]
    IntDefaultOutOfRange {
        field: String,
        value: i128,
        min: i128,
        max: i128,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: `(delta)` can only be applied to integer array elements (u8/u16/u32/u64/i8/i16/i32/i64), not `{type_desc}`"
    )]
    InvalidDeltaElementType {
        type_desc: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: reserved variant name `{name}` cannot be used in type `{type_name}`"
    )]
    ReservedVariantName {
        name: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: lazy field '{field}' added via diff must be optional (`lazy T?`) so older messages can default to None"
    )]
    LazyDiffFieldMustBeOptional {
        field: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("unknown import alias '{alias}'")]
    UnknownImportAlias {
        alias: String,
        span: Span,
        line: u32,
    },

    #[error(
        "import '{alias}' references version {version} but schema only has versions up to {max}"
    )]
    ImportVersionOutOfRange {
        alias: String,
        version: i128,
        max: i128,
        span: Span,
        line: u32,
    },

    #[error("import path '{path}' could not be found or read")]
    ImportNotFound { path: String, span: Span, line: u32 },

    #[error("circular import detected: '{path}'")]
    CircularImport { path: String, span: Span, line: u32 },

    // just the inner error message — path is shown by the renderer
    #[error("{src}")]
    ImportParseFailed {
        path: String,
        src: String,
        span: Span,
        line: u32,
    },

    #[error("schema must have at least one version")]
    NoVersions { span: Span, line: u32 },
}

impl IndexableError for AnalysisError {
    fn span(&self) -> Span {
        match self {
            AnalysisError::UnknownType { span, .. } => *span,
            AnalysisError::UnknownParentType { span, .. } => *span,
            AnalysisError::UnknownEnumVariant { span, .. } => *span,
            AnalysisError::ExtendsWithFullDefinition { span, .. } => *span,
            AnalysisError::FieldNotFound { span, .. } => *span,
            AnalysisError::MissingDefault { span, .. } => *span,
            AnalysisError::FixedSizeDefaultLengthMismatch { span, .. } => *span,
            AnalysisError::FieldAlreadyExists { span, .. } => *span,
            AnalysisError::FixedStringDefaultLengthMismatch { span, .. } => *span,
            AnalysisError::FixedSizeTooLarge { span, .. } => *span,
            AnalysisError::TypeMismatch { span, .. } => *span,
            AnalysisError::VarintsCannotBeConst { span, .. } => *span,
            AnalysisError::InvalidVFloat { span, .. } => *span,
            AnalysisError::VFloatRangeTooLarge { span, .. } => *span,
            AnalysisError::VFloatDefaultOutOfRange { span, .. } => *span,
            AnalysisError::IntDefaultOutOfRange { span, .. } => *span,
            AnalysisError::InvalidDeltaElementType { span, .. } => *span,
            AnalysisError::ReservedVariantName { span, .. } => *span,
            AnalysisError::LazyDiffFieldMustBeOptional { span, .. } => *span,
            AnalysisError::UnknownImportAlias { span, .. } => *span,
            AnalysisError::ImportVersionOutOfRange { span, .. } => *span,
            AnalysisError::ImportNotFound { span, .. } => *span,
            AnalysisError::CircularImport { span, .. } => *span,
            AnalysisError::ImportParseFailed { span, .. } => *span,
            AnalysisError::NoVersions { span, .. } => *span,
        }
    }

    fn line(&self) -> u32 {
        match self {
            AnalysisError::UnknownType { line, .. } => *line,
            AnalysisError::UnknownParentType { line, .. } => *line,
            AnalysisError::UnknownEnumVariant { line, .. } => *line,
            AnalysisError::ExtendsWithFullDefinition { line, .. } => *line,
            AnalysisError::FieldNotFound { line, .. } => *line,
            AnalysisError::MissingDefault { line, .. } => *line,
            AnalysisError::FixedSizeDefaultLengthMismatch { line, .. } => *line,
            AnalysisError::FieldAlreadyExists { line, .. } => *line,
            AnalysisError::FixedStringDefaultLengthMismatch { line, .. } => *line,
            AnalysisError::FixedSizeTooLarge { line, .. } => *line,
            AnalysisError::TypeMismatch { line, .. } => *line,
            AnalysisError::VarintsCannotBeConst { line, .. } => *line,
            AnalysisError::InvalidVFloat { line, .. } => *line,
            AnalysisError::VFloatRangeTooLarge { line, .. } => *line,
            AnalysisError::VFloatDefaultOutOfRange { line, .. } => *line,
            AnalysisError::IntDefaultOutOfRange { line, .. } => *line,
            AnalysisError::InvalidDeltaElementType { line, .. } => *line,
            AnalysisError::ReservedVariantName { line, .. } => *line,
            AnalysisError::LazyDiffFieldMustBeOptional { line, .. } => *line,
            AnalysisError::UnknownImportAlias { line, .. } => *line,
            AnalysisError::ImportVersionOutOfRange { line, .. } => *line,
            AnalysisError::ImportNotFound { line, .. } => *line,
            AnalysisError::CircularImport { line, .. } => *line,
            AnalysisError::ImportParseFailed { line, .. } => *line,
            AnalysisError::NoVersions { line, .. } => *line,
        }
    }
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error(transparent)]
    Lex(#[from] LexError),
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Analysis(#[from] AnalysisError),
}
