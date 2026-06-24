use crate::lexer::Token;
use crate::span::Span;
use thiserror::Error;

pub trait IndexableError {
    fn span(&self) -> Span;
    fn line(&self) -> u32;
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Unexpected token `{got}`, expected {expected}, at line {line}")]
    UnexpectedToken {
        got: Token,
        expected: &'static str,
        span: Span,
        line: u32,
    },
    #[error("Unexpected EOF")]
    UnexpectedEof,
    #[error("Invalid syntax at line {line}: `{message}`")]
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
    #[error("Unexpected character `{ch}`, line {line}")]
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
    #[error("unknown type '{name}' referenced in version {version}, line {line}")]
    UnknownType {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("type '{child}' in version {version} extends unknown type '{parent}', line {line}")]
    UnknownParentType {
        child: String,
        parent: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: enum '{type_name}' has no variant '{variant}', line {line}")]
    UnknownEnumVariant {
        type_name: String,
        variant: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "type '{name}' in version {version} uses 'extends' but body must be a diff, line {line}"
    )]
    ExtendsWithFullDefinition {
        name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "diff op '{op}' references unknown field '{field}' in type '{type_name}' version {version}, line {line}"
    )]
    FieldNotFound {
        op: &'static str,
        field: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("field '{field}' in version {version} doesn't have a default value, line {line}")]
    MissingDefault {
        field: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version} has a fixed {kind} of {expected} entries but default value has {got} entries, line {line}"
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

    #[error("version {version}: diff adds field `{field}` but it already exists, line {line}")]
    FieldAlreadyExists {
        version: i128,
        field: String,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version} has a fixed string of {expected} bytes but default value is {got} bytes, line {line}"
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
        "version {version}: {kind} fixed size {n} is too large, consider not using a fixed size, line {line}"
    )]
    FixedSizeTooLarge {
        kind: &'static str,
        n: usize,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: type mismatch: expected {expected}, got {got}, line {line}")]
    TypeMismatch {
        expected: String,
        got: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: varints cannot be used in const fields, line {line}")]
    VarintsCannotBeConst {
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: invalid vfloat: {reason}, line {line}")]
    InvalidVFloat {
        reason: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("version {version}: vfloat range is too large: {range}, line {line}")]
    VFloatRangeTooLarge {
        range: f64,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max}), line {line}"
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
        "field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max}), line {line}"
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
        "version {version}: `(delta)` can only be applied to integer array elements (u8/u16/u32/u64/i8/i16/i32/i64), not `{type_desc}`, line {line}"
    )]
    InvalidDeltaElementType {
        type_desc: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: reserved variant name `{name}` cannot be used in type `{type_name}`, line {line}"
    )]
    ReservedVariantName {
        name: String,
        type_name: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error(
        "version {version}: lazy field '{field}' added via diff must be optional (`lazy T?`) so older messages can default to None, line {line}"
    )]
    LazyDiffFieldMustBeOptional {
        field: String,
        version: i128,
        span: Span,
        line: u32,
    },

    #[error("unknown import alias '{alias}', line {line}")]
    UnknownImportAlias {
        alias: String,
        span: Span,
        line: u32,
    },

    #[error(
        "import '{alias}' references version {version} but schema only has versions up to {max}, line {line}"
    )]
    ImportVersionOutOfRange {
        alias: String,
        version: i128,
        max: i128,
        span: Span,
        line: u32,
    },

    #[error("import path '{path}' could not be found or read, line {line}")]
    ImportNotFound { path: String, span: Span, line: u32 },

    #[error("circular import detected: '{path}', line {line}")]
    CircularImport { path: String, span: Span, line: u32 },

    #[error("failed to parse schema '{path}': {src}, line {line}")]
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
