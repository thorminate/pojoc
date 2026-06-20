use std::fmt;

#[derive(Debug, PartialEq)]
pub enum Error {
    /// Buffer ended before we finished reading
    UnexpectedEof,
    /// A varint had too many continuation bytes (>10 for u64)
    VarIntOverflow,
    /// The enum variant was not recognized
    InvalidEnumVariant,
    /// The message envelope had an unrecognized version
    UnsupportedVersion(u64),
    /// The declared payload length exceeds what's in the buffer
    InvalidLength,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEof => write!(f, "unexpected end of buffer"),
            Error::VarIntOverflow => write!(f, "varint overflowed u64"),
            Error::InvalidEnumVariant => write!(f, "invalid enum variant"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported message version: {v}"),
            Error::InvalidLength => write!(f, "declared length exceeds buffer"),
        }
    }
}

impl std::error::Error for Error {}

pub type PojocResult<T> = Result<T, Error>;
