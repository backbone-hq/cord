use serde::{de, ser};
use std::fmt::Display;
use thiserror::Error;

pub type CordResult<T, E = CordError> = std::result::Result<T, E>;

#[derive(Debug, Error, PartialEq)]
pub enum CordError {
    #[error("Cord encountered I/O error: {0}")]
    IOError(String),
    #[error("Cord does not support: {0}")]
    NotSupported(&'static str),
    #[error("Cord could not validate: {0}")]
    ValidationError(&'static str),
    #[error("Cord serialization error: {0}")]
    SerializationError(String),
    #[error("Cord deserialization error: {0}")]
    DeserializationError(String),
    #[error("Cord schema error: {0}")]
    SchemaError(String),
    #[error("Cord depth limit exceeded")]
    DepthLimitExceeded,
    #[error("Cord length limit exceeded: {0} exceeds maximum of {1}")]
    LengthLimitExceeded(usize, usize),
    #[error("Cord encountered unknown enum variant index {0}")]
    UnknownVariant(u32),
    #[error("Duplicate key in map")]
    DuplicateMapKey,
    #[error("Duplicate element in set")]
    DuplicateSetElement,
    #[error("Trailing bytes after deserialization")]
    TrailingBytes,
    #[error("Unexpected end of input")]
    UnexpectedEof,
    #[error("Invalid boolean value")]
    InvalidBooleanValue,
    #[error("Invalid varint encoding")]
    InvalidVarInt,
    #[error("Invalid UTF-8")]
    InvalidUtf8,
    #[error("String is not NFC normalized")]
    NotNfcNormalized,
    #[error("NaN is not allowed in canonical encoding")]
    NanNotAllowed,
    #[error("Negative zero is not allowed in canonical encoding")]
    NegativeZeroNotAllowed,
    #[error("Conflicting encoding hints")]
    ConflictingEncodingHints,
    #[error("Value overflow for encoding width")]
    Overflow,
}

impl From<std::io::Error> for CordError {
    fn from(err: std::io::Error) -> Self {
        CordError::IOError(err.to_string())
    }
}

impl ser::Error for CordError {
    fn custom<T: Display>(msg: T) -> Self {
        CordError::SerializationError(msg.to_string())
    }
}

impl de::Error for CordError {
    fn custom<T: Display>(msg: T) -> Self {
        CordError::DeserializationError(msg.to_string())
    }
}
