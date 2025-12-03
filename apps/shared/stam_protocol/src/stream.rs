use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{ProtocolError, Result};
use crate::primal_message::PrimalMessage;
use crate::game_message::GameMessage;

/// Default maximum message size: 25MB (configurable via network_max_chunk_size)
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 25 * 1024 * 1024;

/// Extension trait for TcpStream to read/write PrimalMessages
pub trait PrimalStream {
    /// Read a PrimalMessage from the stream with default max size
    /// Format: [4 bytes length][message bytes]
    async fn read_primal_message(&mut self) -> Result<PrimalMessage>;

    /// Read a PrimalMessage from the stream with custom max size
    /// Format: [4 bytes length][message bytes]
    async fn read_primal_message_with_max_size(&mut self, max_size: usize) -> Result<PrimalMessage>;

    /// Write a PrimalMessage to the stream with default max size
    /// Format: [4 bytes length][message bytes]
    async fn write_primal_message(&mut self, message: &PrimalMessage) -> Result<()>;

    /// Write a PrimalMessage to the stream with custom max size
    /// Format: [4 bytes length][message bytes]
    async fn write_primal_message_with_max_size(&mut self, message: &PrimalMessage, max_size: usize) -> Result<()>;

    /// Write a raw data chunk directly without extra allocations
    /// Format: [4 bytes total_len][1 byte is_final][4 bytes data_len][data bytes]
    /// This is optimized for streaming large files - avoids serialization overhead
    async fn write_raw_chunk(&mut self, data: &[u8], is_final: bool) -> Result<()>;

    /// Read a raw data chunk into a pre-allocated buffer, returning bytes read and is_final flag
    /// Returns (bytes_read, is_final). The data is written to the provided buffer.
    /// This is optimized for streaming large files - avoids deserialization overhead
    async fn read_raw_chunk(&mut self, buffer: &mut [u8]) -> Result<(usize, bool)>;
}

impl PrimalStream for TcpStream {
    async fn read_primal_message(&mut self) -> Result<PrimalMessage> {
        self.read_primal_message_with_max_size(DEFAULT_MAX_MESSAGE_SIZE).await
    }

    async fn read_primal_message_with_max_size(&mut self, max_size: usize) -> Result<PrimalMessage> {
        // Read message length (4 bytes, big-endian)
        let len = self.read_u32().await? as usize;

        // Check max size
        if len > max_size {
            return Err(ProtocolError::MessageTooLarge(len, max_size));
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
        if data.len() > DEFAULT_MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge(data.len(), DEFAULT_MAX_MESSAGE_SIZE));
        }

        // Write length (4 bytes, big-endian)
        self.write_u32(data.len() as u32).await?;

        // Write message data
        self.write_all(&data).await?;

        // Standard messages are flushed immediately for request-response patterns
        self.flush().await?;

        Ok(())
    }

    async fn write_primal_message_with_max_size(&mut self, message: &PrimalMessage, max_size: usize) -> Result<()> {
        // Serialize message
        let data = message.to_bytes()?;

        // Check max size
        if data.len() > max_size {
            return Err(ProtocolError::MessageTooLarge(data.len(), max_size));
        }

        // Write length (4 bytes, big-endian)
        self.write_u32(data.len() as u32).await?;

        // Write message data
        self.write_all(&data).await?;

        // Large messages (e.g., file chunks) don't flush for better throughput
        // TCP handles buffering, and the final chunk or next message will flush

        Ok(())
    }

    async fn write_raw_chunk(&mut self, data: &[u8], is_final: bool) -> Result<()> {
        // Format: [4 bytes total_len][1 byte is_final][4 bytes data_len][data bytes]
        // total_len = 1 + 4 + data.len()
        let total_len = 1 + 4 + data.len();

        // Write total length
        self.write_u32(total_len as u32).await?;

        // Write is_final flag (1 byte)
        self.write_u8(if is_final { 1 } else { 0 }).await?;

        // Write data length
        self.write_u32(data.len() as u32).await?;

        // Write data directly from the slice - no allocation!
        self.write_all(data).await?;

        // Don't flush - let TCP buffer for throughput
        Ok(())
    }

    async fn read_raw_chunk(&mut self, buffer: &mut [u8]) -> Result<(usize, bool)> {
        // Format: [4 bytes total_len][1 byte is_final][4 bytes data_len][data bytes]

        // Read total length
        let total_len = self.read_u32().await? as usize;

        if total_len < 5 {
            return Err(ProtocolError::ConnectionClosed);
        }

        // Read is_final flag
        let is_final = self.read_u8().await? != 0;

        // Read data length
        let data_len = self.read_u32().await? as usize;

        // Validate buffer is large enough
        if data_len > buffer.len() {
            return Err(ProtocolError::MessageTooLarge(data_len, buffer.len()));
        }

        // Read data directly into the provided buffer - no allocation!
        self.read_exact(&mut buffer[..data_len]).await?;

        Ok((data_len, is_final))
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
        if len > DEFAULT_MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge(len, DEFAULT_MAX_MESSAGE_SIZE));
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
        if data.len() > DEFAULT_MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge(data.len(), DEFAULT_MAX_MESSAGE_SIZE));
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
