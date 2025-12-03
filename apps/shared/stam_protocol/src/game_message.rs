use serde::{Deserialize, Serialize};

/// Mod information sent to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    /// Mod ID (unique identifier)
    pub mod_id: String,
    /// Mod type (e.g., "bootstrap", "library", etc.)
    pub mod_type: String,
    /// Download URL for this mod
    pub download_url: String,
    /// SHA-512 hash of the mod archive
    pub archive_sha512: String,
    /// Size of the mod archive in bytes
    pub archive_bytes: u64,
    /// Size of the uncompressed mod contents in bytes
    pub uncompressed_bytes: u64,
}

/// Game protocol messages for authenticated game clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameMessage {
    // Server -> Client messages
    /// Authentication successful (sent immediately after GameLogin Intent)
    LoginSuccess {
        /// Human-readable game name
        game_name: String,
        /// Game version from server configuration
        game_version: String,
        /// List of required mods for this game
        mods: Vec<ModInfo>,
    },

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
