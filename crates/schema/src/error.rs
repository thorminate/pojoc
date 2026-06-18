use crate::lexer::Token;
use crate::span::Span;
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum LexError {
    #[error("Unexpected character `{ch}`, line {line}")]
    UnexpectedChar { ch: char, span: Span, line: u32 },
}

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("unknown type '{name}' referenced in version {version}, line {line}")]
    UnknownType { name: String, version: i128, span: Span, line: u32 },

    #[error("type '{child}' in version {version} extends unknown type '{parent}', line {line}")]
    UnknownParentType { child: String, parent: String, version: i128, span: Span, line: u32 },

    #[error("type '{name}' in version {version} uses 'extends' but body must be a diff, line {line}")]
    ExtendsWithFullDefinition { name: String, version: i128, span: Span, line: u32 },

    #[error("diff op '{op}' references unknown field '{field}' in type '{type_name}' version {version}, line {line}")]
    FieldNotFound { op: &'static str, field: String, type_name: String, version: i128, span: Span, line: u32 },

    #[error("field '{field}' in version {version} doesn't have a default value, line {line}")]
    MissingDefault { field: String, version: i128, span: Span, line: u32 },

    // was a tuple variant `(i128, String)`; given a struct shape now that it carries span/line too.
    #[error("version {version}: diff adds field `{field}` but it already exists, line {line}")]
    FieldAlreadyExists { version: i128, field: String, span: Span, line: u32 },

    #[error("field '{field}' in version {version} has a fixed string of {expected} bytes but default value is {got} bytes, line {line}")]
    FixedStringDefaultLengthMismatch { field: String, expected: usize, got: usize, version: i128, span: Span, line: u32 },

    #[error("version {version}: {kind} fixed size {n} is too large, consider not using a fixed size, line {line}")]
    FixedSizeTooLarge { kind: &'static str, n: usize, version: i128, span: Span, line: u32 },

    #[error("version {version}: type mismatch: expected {expected}, got {got}, line {line}")]
    TypeMismatch { expected: String, got: String, version: i128, span: Span, line: u32 },

    #[error("version {version}: varints cannot be used in const fields, line {line}")]
    VarintsCannotBeConst { version: i128, span: Span, line: u32 },

    #[error("version {version}: invalid vfloat: {reason}, line {line}")]
    InvalidVFloat { reason: String, version: i128, span: Span, line: u32 },

    // renamed `span: f64` -> `range: f64` to avoid colliding with the new Span type.
    #[error("version {version}: vfloat range is too large: {range}, line {line}")]
    VFloatRangeTooLarge { range: f64, version: i128, span: Span, line: u32 },

    #[error("field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max}), line {line}")]
    VFloatDefaultOutOfRange { field: String, value: f64, min: f64, max: f64, version: i128, span: Span, line: u32 },

    #[error("version {version}: `(delta)` can only be applied to integer array elements (u8/u16/u32/u64/i8/i16/i32/i64), not `{type_desc}`, line {line}")]
    InvalidDeltaElementType { type_desc: String, version: i128, span: Span, line: u32 },

    #[error("version {version}: reserved variant name `{name}` cannot be used in type `{type_name}`, line {line}")]
    ReservedVariantName { name: String, type_name: String, version: i128, span: Span, line: u32 },

    #[error("version {version}: lazy field '{field}' added via diff must be optional (`lazy T?`) so older messages can default to None, line {line}")]
    LazyDiffFieldMustBeOptional { field: String, version: i128, span: Span, line: u32 },

    // no field/version context exists for "zero versions" — point at the schema's own span.
    #[error("schema must have at least one version")]
    NoVersions { span: Span, line: u32 },
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