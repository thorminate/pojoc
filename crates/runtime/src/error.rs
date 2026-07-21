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
    /// A `min:`/`max:` constrained field's value (or, for a string/array/map,
    /// its length/element count) fell outside its declared bounds.
    ConstraintViolation {
        field: &'static str,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// An `intern`-marked field's varint table index had no corresponding
    /// entry in the message's string-interning table.
    InvalidInternIndex,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEof => write!(f, "unexpected end of buffer"),
            Error::VarIntOverflow => write!(f, "varint overflowed u64"),
            Error::InvalidEnumVariant => write!(f, "invalid enum variant"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported message version: {v}"),
            Error::InvalidLength => write!(f, "declared length exceeds buffer"),
            Error::ConstraintViolation { field, min, max } => {
                write!(f, "field `{field}` violated its constraint (min: {min:?}, max: {max:?})")
            }
            Error::InvalidInternIndex => write!(f, "string-interning table index out of range"),
        }
    }
}

impl std::error::Error for Error {}

pub type PojocResult<T> = Result<T, Error>;
