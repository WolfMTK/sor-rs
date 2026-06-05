use thiserror::Error;

/// Library errors.
#[derive(Debug, Error)]
pub enum SorError {
    #[error("SOR parsing error: {0}")]
    ParseError(String),

    #[error("Checksum mismatch: {expected:#06x}, got {actual:#06x}")]
    ChecksumError { expected: u16, actual: u16 },

    #[error("I/O error: {0}")]
    IOError(#[from] std::io::Error),
}

impl SorError {
    pub(crate) fn parse(msg: impl Into<String>) -> Self {
        SorError::ParseError(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, SorError>;
