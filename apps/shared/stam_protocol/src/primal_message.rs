use serde::{Deserialize, Serialize};

/// Server information for server list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Game ID (unique identifier for the game)
    pub game_id: String,
    /// Game name (human-readable)
    pub name: String,
    /// Server URI (e.g., "stam://game.example.com:9999")
    pub uri: String,
}

/// Client intent type - determines how the connection will be handled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentType {
    /// Primal login - for authentication and server list
    PrimalLogin,
    /// Game login - for game client connections
    GameLogin,
    /// Server login - for server-to-server connections
    ServerLogin,
    /// URI request - one-shot request for downloading resources via stam:// protocol
    RequestUri,
}

/// Primal protocol messages for initial connection handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrimalMessage {
    /// Stub message for testing
    Stub,

    // Server -> Client messages
    /// Welcome message with server version
    Welcome {
        /// Server version string
        version: String,
    },

    /// Error message from server to client (causes immediate disconnection)
    Error {
        /// Error message
        message: String,
    },

    /// Disconnect message from server to client (graceful disconnection)
    Disconnect {
        /// Disconnect reason/message
        message: String,
    },

    /// Server list sent from server to client after successful PrimalLogin
    ServerList {
        /// List of available game servers
        servers: Vec<ServerInfo>,
    },

    // Client -> Server messages
    /// Client intent - declares connection type and provides credentials
    Intent {
        /// Type of connection intent
        intent_type: IntentType,
        /// Client version for compatibility check
        client_version: String,
        /// Username
        username: String,
        /// SHA-512 hash of the password (not plaintext)
        password_hash: String,
        /// Game ID (required for GameLogin and RequestUri, optional for PrimalLogin)
        game_id: Option<String>,
        /// URI being requested (required for RequestUri intent, sanitized without credentials)
        uri: Option<String>,
    },

    // Server -> Client messages for RequestUri
    /// URI response - sent as response to RequestUri intent
    UriResponse {
        /// HTTP status code (200 = success, 404 = not found, 500 = error, etc.)
        status: u16,
        /// Response buffer data (if any)
        buffer: Option<Vec<u8>>,
        /// File name (if response is a file)
        file_name: Option<String>,
        /// File size in bytes (if response is a file)
        file_size: Option<u64>,
    },

    /// URI response chunk - sent for large file transfers
    UriResponseChunk {
        /// Chunk data
        data: Vec<u8>,
        /// Whether this is the final chunk
        is_final: bool,
    },
}

impl PrimalMessage {
    /// Serialize message to bytes using bincode
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize message from bytes using bincode
    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}
