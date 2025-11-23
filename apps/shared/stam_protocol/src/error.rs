use thiserror::Error;

pub type Result<T> = std::result::Result<T, ProtocolError>;

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Message too large: {0} bytes (max: {1})")]
    MessageTooLarge(usize, usize),

    #[error("Connection closed")]
    ConnectionClosed,
}
