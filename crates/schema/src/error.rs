use crate::lexer::Token;

#[derive(Debug)]
pub enum ParseError {
    UnexpectedToken { got: Token, expected: &'static str, pos: usize },
    UnexpectedEof,
    InvalidSyntax(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ParseError::UnexpectedToken { got, expected, pos } => write!(f, "Unexpected token `{}`, expected {}, got {}", got, expected, *pos),
            ParseError::UnexpectedEof => write!(f, "Unexpected EOF"),
            ParseError::InvalidSyntax(s) => write!(f, "Invalid syntax `{}`", s),
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
            LexError::UnexpectedChar(c, pos) => write!(f, "Unexpected character `{}`, pos {}", c, pos),
        }
    }
}

impl std::error::Error for LexError {}

#[derive(Debug)]
pub enum AnalysisError {
    UnknownType { name: String, version: u32 },
    UnknownParentType { child: String, parent: String, version: u32 },
    ExtendsWithFullDefinition { name: String, version: u32 },
    FieldNotFound { op: &'static str, field: String, type_name: String, version: u32 },
    MissingDefault { field: String, version: u32 },
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