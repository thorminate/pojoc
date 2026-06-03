use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    UnexpectedToken,
    UnexpectedEof,
    InvalidSyntax(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedToken => write!(f, "unexpected token"),
            ParseError::UnexpectedEof => write!(f, "unexpected EOF"),
            ParseError::InvalidSyntax(name) => write!(f, "invalid syntax: {name}"),
        }
    }
}

impl std::error::Error for ParseError {}