use serde::{Deserialize, Serialize};

/// Game protocol messages for authenticated game clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameMessage {
    // Server -> Client messages
    /// Authentication successful (sent immediately after GameLogin Intent)
    LoginSuccess,

    /// Error message
    Error {
        /// Error message
        message: String,
    },

    /// Disconnect message from server to client (graceful disconnection)
    Disconnect {
        /// Disconnect reason/message
        message: String,
    },

    // Future game messages will be added here
    // Client -> Server:
    // PlayerMove { x: f32, y: f32 },
    // PlayerAction { action: String },
    // Server -> Client:
    // WorldState { ... },
    // PlayerUpdate { ... },
    // etc.
}

impl GameMessage {
    /// Serialize message to bytes using bincode
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize message from bytes using bincode
    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}
