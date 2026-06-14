use crate::lexer::Token;

#[derive(Debug)]
pub enum ParseError {
    UnexpectedToken {
        got: Token,
        expected: &'static str,
        line: usize,
    },
    UnexpectedEof,
    InvalidSyntax(String, usize),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ParseError::UnexpectedToken { got, expected, line } => write!(
                f,
                "Unexpected token `{}`, expected {}, at line {}",
                got, expected, *line
            ),
            ParseError::UnexpectedEof => write!(f, "Unexpected EOF"),
            ParseError::InvalidSyntax(s, line) => write!(f, "Invalid syntax at line {line}: `{s}`"),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug)]
pub enum LexError {
    UnexpectedChar(char, usize),
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LexError::UnexpectedChar(c, pos) => {
                write!(f, "Unexpected character `{}`, pos {}", c, pos)
            }
        }
    }
}

impl std::error::Error for LexError {}

#[derive(Debug)]
pub enum AnalysisError {
    UnknownType {
        name: String,
        version: i128,
    },
    UnknownParentType {
        child: String,
        parent: String,
        version: i128,
    },
    ExtendsWithFullDefinition {
        name: String,
        version: i128,
    },
    FieldNotFound {
        op: &'static str,
        field: String,
        type_name: String,
        version: i128,
    },
    MissingDefault {
        field: String,
        version: i128,
    },
    FieldAlreadyExists(i128, String),
    FixedStringDefaultLengthMismatch {
        field: String,
        expected: usize,
        got: usize,
        version: i128,
    },
    FixedSizeTooLarge {
        kind: &'static str,
        n: usize,
        version: i128,
    },
    TypeMismatch {
        expected: String,
        got: String,
        version: i128,
    },
    VarintsCannotBeConst {
        version: i128
    },
    InvalidVFloat {
        reason: String,
        version: i128,
    },
    VFloatRangeTooLarge {
        span: f64,
        version: i128,
    },
    VFloatDefaultOutOfRange {
        field: String,
        value: f64,
        min: f64,
        max: f64,
        version: i128,
    },
    InvalidDeltaElementType {
        type_desc: String,
        version: i128,
    },
    ReservedVariantName { name: String, type_name: String, version: i128 },
    NoVersions,
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AnalysisError::UnknownType { name, version } =>
                write!(f, "unknown type '{}' referenced in version {}", name, version),
            AnalysisError::UnknownParentType { child, parent, version } =>
                write!(f, "type '{}' in version {} extends unknown type '{}'", child, version, parent),
            AnalysisError::ExtendsWithFullDefinition { name, version } =>
                write!(f, "type '{}' in version {} uses 'extends' but body must be a diff", name, version),
            AnalysisError::FieldNotFound { op, field, type_name, version } =>
                write!(f, "diff op '{op}' references unknown field '{field}' in type '{type_name}' version {version}"),
            AnalysisError::MissingDefault { field, version } =>
                write!(f, "field '{field}' in version {version} doesn't have a default value"),
            AnalysisError::FieldAlreadyExists(v, s) =>
                write!(f, "version {v}: diff adds field `{s}` but it already exists"),
            AnalysisError::FixedStringDefaultLengthMismatch { field, expected, got, version } =>
                write!(f, "field '{field}' in version {version} has a fixed string of {expected} bytes \
                but default value is {got} bytes"),
            AnalysisError::FixedSizeTooLarge { kind, n, version } =>
                write!(f, "version {version}: {kind} fixed size {n} is too large, consider not using a fixed size."),
            AnalysisError::TypeMismatch { expected, got, version } =>
                write!(f, "version {version}: type mismatch: expected {expected}, got {got}"),
            AnalysisError::VarintsCannotBeConst { version } =>
                write!(f, "version {version}: varints cannot be used in const fields"),
            AnalysisError::InvalidVFloat { reason, version } =>
                write!(f, "version {version}: invalid vfloat: {reason}"),
            AnalysisError::VFloatRangeTooLarge { span, version } =>
                write!(f, "version {version}: vfloat range is too large: {span}"),
            AnalysisError::VFloatDefaultOutOfRange { field, value, min, max, version } =>
                write!(f, "field '{field}' in version {version}'s default value {value} is out of range (min: {min}, max: {max})"),
            AnalysisError::InvalidDeltaElementType { type_desc, version } => 
                write!(f, "version {version}: `(delta)` can only be applied to integer array elements (u8/u16/u32/u64/i8/i16/i32/i64), not `{type_desc}`"),
            AnalysisError::ReservedVariantName { name, type_name, version } =>
            write!(f, "version {version}: reserved variant name `{name}` cannot be used in type `{type_name}`"),
            AnalysisError::NoVersions =>
                write!(f, "schema must have at least one version"),
        }
    }
}

impl std::error::Error for AnalysisError {}

#[derive(Debug)]
pub enum SchemaError {
    Lex(LexError),
    Parse(ParseError),
    Analysis(AnalysisError),
}

impl From<LexError> for SchemaError {
    fn from(e: LexError) -> Self {
        SchemaError::Lex(e)
    }
}

impl From<ParseError> for SchemaError {
    fn from(e: ParseError) -> Self {
        SchemaError::Parse(e)
    }
}

impl From<AnalysisError> for SchemaError {
    fn from(e: AnalysisError) -> Self {
        SchemaError::Analysis(e)
    }
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SchemaError::Lex(e) => write!(f, "{e}"),
            SchemaError::Parse(e) => write!(f, "{e}"),
            SchemaError::Analysis(e) => write!(f, "{e}"),
        }
    }
}
