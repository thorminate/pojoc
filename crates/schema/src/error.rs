use crate::lexer::Token;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Unexpected token `{got}`, expected {expected}, at line {line}")]
    UnexpectedToken {
        got: Token,
        expected: &'static str,
        line: usize,
    },
    #[error("Unexpected EOF")]
    UnexpectedEof,
    #[error("Invalid syntax at line {1}: `{0}`")]
    InvalidSyntax(String, usize),
}

#[derive(Debug, Error)]
pub enum LexError {
    #[error("Unexpected character `{0}`, pos {1}")]
    UnexpectedChar(char, usize),
}

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("unknown type '{name}' referenced in version {version}")]
    UnknownType { name: String, version: i128 },

    #[error("type '{child}' in version {version} extends unknown type '{parent}'")]
    UnknownParentType {
        child: String,
        parent: String,
        version: i128,
    },

    #[error("type '{name}' in version {version} uses 'extends' but body must be a diff")]
    ExtendsWithFullDefinition { name: String, version: i128 },

    #[error("diff op '{op}' references unknown field '{field}' in type '{type_name}' version {version}")]
    FieldNotFound {
        op: &'static str,
        field: String,
        type_name: String,
        version: i128,
    },

    #[error("field '{field}' in version {version} doesn't have a default value")]
    MissingDefault { field: String, version: i128 },

    #[error("version {0}: diff adds field `{1}` but it already exists")]
    FieldAlreadyExists(i128, String),

    #[error("field '{field}' in version {version} has a fixed string of {expected} bytes but default value is {got} bytes")]
    FixedStringDefaultLengthMismatch {
        field: String,
        expected: usize,
        got: usize,
        version: i128,
    },

    #[error("version {version}: {kind} fixed size {n} is too large, consider not using a fixed size.")]
    FixedSizeTooLarge {
        kind: &'static str,
        n: usize,
        version: i128,
    },

    #[error("version {version}: type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        expected: String,
        got: String,
        version: i128,
    },

    #[error("version {version}: varints cannot be used in const fields")]
    VarintsCannotBeConst { version: i128 },

    #[error("version {version}: invalid vfloat: {reason}")]
    InvalidVFloat { reason: String, version: i128 },

    #[error("version {version}: vfloat range is too large: {span}")]
    VFloatRangeTooLarge { span: f64, version: i128 },

    #[error("field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max})")]
    VFloatDefaultOutOfRange {
        field: String,
        value: f64,
        min: f64,
        max: f64,
        version: i128,
    },

    #[error("version {version}: `(delta)` can only be applied to integer array elements (u8/u16/u32/u64/i8/i16/i32/i64), not `{type_desc}`")]
    InvalidDeltaElementType { type_desc: String, version: i128 },

    #[error("version {version}: reserved variant name `{name}` cannot be used in type `{type_name}`")]
    ReservedVariantName {
        name: String,
        type_name: String,
        version: i128,
    },

    #[error("version {version}: lazy field '{field}' added via diff must be optional (`lazy T?`) so older messages can default to None")]
    LazyDiffFieldMustBeOptional { field: String, version: i128 },

    #[error("schema must have at least one version")]
    NoVersions,
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