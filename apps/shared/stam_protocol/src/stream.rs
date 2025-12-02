use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{ProtocolError, Result};
use crate::primal_message::PrimalMessage;
use crate::game_message::GameMessage;

/// Maximum message size: 2MB (to accommodate 512KB chunks with serialization overhead)
const MAX_MESSAGE_SIZE: usize = 2 * 1024 * 1024;

/// Extension trait for TcpStream to read/write PrimalMessages
pub trait PrimalStream {
    /// Read a PrimalMessage from the stream
    /// Format: [4 bytes length][message bytes]
    async fn read_primal_message(&mut self) -> Result<PrimalMessage>;

    /// Write a PrimalMessage to the stream
    /// Format: [4 bytes length][message bytes]
    async fn write_primal_message(&mut self, message: &PrimalMessage) -> Result<()>;
}

impl PrimalStream for TcpStream {
    async fn read_primal_message(&mut self) -> Result<PrimalMessage> {
        // Read message length (4 bytes, big-endian)
        let len = self.read_u32().await? as usize;

        // Check max size
        if len > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge(len, MAX_MESSAGE_SIZE));
        }

        if len == 0 {
            return Err(ProtocolError::ConnectionClosed);
        }

        // Read message data
        let mut buffer = vec![0u8; len];
        self.read_exact(&mut buffer).await?;

        // Deserialize
        PrimalMessage::from_bytes(&buffer).map_err(Into::into)
    }

    async fn write_primal_message(&mut self, message: &PrimalMessage) -> Result<()> {
        // Serialize message
        let data = message.to_bytes()?;

        // Check max size
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge(data.len(), MAX_MESSAGE_SIZE));
        }

        // Write length (4 bytes, big-endian)
        self.write_u32(data.len() as u32).await?;

        // Write message data
        self.write_all(&data).await?;

        // Flush to ensure data is sent
        self.flush().await?;

        Ok(())
    }
}

/// Extension trait for TcpStream to read/write GameMessages
pub trait GameStream {
    /// Read a GameMessage from the stream
    /// Format: [4 bytes length][message bytes]
    async fn read_game_message(&mut self) -> Result<GameMessage>;

    /// Write a GameMessage to the stream
    /// Format: [4 bytes length][message bytes]
    async fn write_game_message(&mut self, message: &GameMessage) -> Result<()>;
}

impl GameStream for TcpStream {
    async fn read_game_message(&mut self) -> Result<GameMessage> {
        // Read message length (4 bytes, big-endian)
        let len = self.read_u32().await? as usize;

        // Check max size
        if len > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge(len, MAX_MESSAGE_SIZE));
        }

        if len == 0 {
            return Err(ProtocolError::ConnectionClosed);
        }

        // Read message data
        let mut buffer = vec![0u8; len];
        self.read_exact(&mut buffer).await?;

        // Deserialize
        GameMessage::from_bytes(&buffer).map_err(Into::into)
    }

    async fn write_game_message(&mut self, message: &GameMessage) -> Result<()> {
        // Serialize message
        let data = message.to_bytes()?;

        // Check max size
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge(data.len(), MAX_MESSAGE_SIZE));
        }

        // Write length (4 bytes, big-endian)
        self.write_u32(data.len() as u32).await?;

        // Write message data
        self.write_all(&data).await?;

        // Flush to ensure data is sent
        self.flush().await?;

        Ok(())
    }
}
