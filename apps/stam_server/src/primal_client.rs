use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::{info, debug, error, warn, trace};

use stam_protocol::{IntentType, PrimalMessage, PrimalStream, ServerInfo};

use crate::game_client::GameClient;
use crate::config::Config;
use crate::client_manager::{ClientManager, ClientType};
use crate::mod_loader::GameModRuntime;
use crate::VERSION;

/// Shared registry of GameModRuntime instances for each game
/// Used for dispatching RequestUri events to mod handlers
pub type GameRuntimes = Arc<HashMap<String, GameModRuntime>>;

/// PrimalClient represents a client connection in its initial state
/// Used for authentication and server list distribution
pub struct PrimalClient {
    /// TCP stream for this client connection
    stream: TcpStream,
    /// Remote address of the client
    addr: SocketAddr,
    /// Server configuration
    config: Config,
    /// Client manager for tracking connections
    client_manager: ClientManager,
    /// Game mod runtimes for event dispatch
    game_runtimes: GameRuntimes,
}

impl PrimalClient {
    /// Create a new PrimalClient from an accepted TCP connection
    pub fn new(
        stream: TcpStream,
        addr: SocketAddr,
        config: Config,
        client_manager: ClientManager,
        game_runtimes: GameRuntimes,
    ) -> Self {
        info!("New client connected from {}", addr);
        Self { stream, addr, config, client_manager, game_runtimes }
    }

    /// Get the client's remote address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Handle the primal client connection
    /// Sends Welcome, waits for Intent, and routes to appropriate handler
    pub async fn handle(mut self) {
        let addr = self.addr;  // Save addr before moves
        let client_manager = self.client_manager.clone();  // Clone client_manager before moves

        // Register as Primal client (temporary, will transition to Game if needed)
        let _command_rx = client_manager.register_client(addr, ClientType::Primal, None).await;
        // Note: PrimalClient connections are short-lived, so we don't need to handle commands

        debug!("Handling client {}", addr);

        // Send Welcome message with server version
        let welcome = PrimalMessage::Welcome {
            version: VERSION.to_string(),
        };

        if let Err(e) = self.stream.write_primal_message(&welcome).await {
            error!("Failed to send Welcome to {}: {}", addr, e);
            client_manager.unregister_client(&addr).await;
            return;
        }

        debug!("Sent Welcome (version {}) to {}", VERSION, addr);

        // Wait for Intent message
        match self.stream.read_primal_message().await {
            Ok(PrimalMessage::Intent { intent_type, client_version, username, password_hash, game_id, uri }) => {
                debug!("Received Intent from {}: {:?}, user={}, client_version={}, game_id={:?}, uri={:?}", addr, intent_type, username, client_version, game_id, uri);

                // Validate client version (major.minor must match server)
                if !self.is_version_compatible(&client_version) {
                    error!("Version mismatch from {}: client={}, server={}", addr, client_version, VERSION);
                    let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                        message: format!("Version incompatible. Server requires {}.x", self.get_major_minor(VERSION)),
                    }).await;
                    client_manager.unregister_client(&addr).await;
                    info!("Client {} disconnected (version mismatch)", addr);
                    return;
                }

                debug!("Client version {} compatible with server {}", client_version, VERSION);

                match intent_type {
                    IntentType::PrimalLogin => {
                        self.handle_primal_login(username, password_hash).await;
                        // Unregister after PrimalLogin completes
                        client_manager.unregister_client(&addr).await;
                        info!("Client {} disconnected", addr);
                    }
                    IntentType::GameLogin => {
                        // Validate game_id is provided and exists
                        if let Some(ref gid) = game_id {
                            if !self.config.games.contains_key(gid) {
                                error!("Invalid game_id '{}' from {}", gid, addr);
                                let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                                    message: format!("Invalid game_id: {}", gid),
                                }).await;
                                client_manager.unregister_client(&addr).await;
                                info!("Client {} disconnected (invalid game_id)", addr);
                                return;
                            }
                        } else {
                            error!("Missing game_id for GameLogin from {}", addr);
                            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                                message: "game_id required for GameLogin".to_string(),
                            }).await;
                            client_manager.unregister_client(&addr).await;
                            info!("Client {} disconnected (missing game_id)", addr);
                            return;
                        }

                        // Unregister as Primal before transitioning to Game
                        client_manager.unregister_client(&addr).await;
                        self.handle_game_login(username, password_hash, game_id.unwrap()).await;
                        info!("Client {} disconnected", addr);
                    }
                    IntentType::ServerLogin => {
                        warn!("ServerLogin not yet implemented from {}", addr);
                        let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                            message: "ServerLogin not implemented yet".to_string(),
                        }).await;
                        client_manager.unregister_client(&addr).await;
                        info!("Client {} disconnected", addr);
                    }
                    IntentType::RequestUri => {
                        // Validate required fields
                        if game_id.is_none() {
                            error!("Missing game_id for RequestUri from {}", addr);
                            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                                message: "game_id required for RequestUri".to_string(),
                            }).await;
                            client_manager.unregister_client(&addr).await;
                            info!("Client {} disconnected (missing game_id)", addr);
                            return;
                        }
                        if uri.is_none() {
                            error!("Missing uri for RequestUri from {}", addr);
                            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                                message: "uri required for RequestUri".to_string(),
                            }).await;
                            client_manager.unregister_client(&addr).await;
                            info!("Client {} disconnected (missing uri)", addr);
                            return;
                        }

                        let gid = game_id.unwrap();
                        if !self.config.games.contains_key(&gid) {
                            error!("Invalid game_id '{}' for RequestUri from {}", gid, addr);
                            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                                message: format!("Invalid game_id: {}", gid),
                            }).await;
                            client_manager.unregister_client(&addr).await;
                            info!("Client {} disconnected (invalid game_id)", addr);
                            return;
                        }

                        self.handle_request_uri(username, password_hash, gid, uri.unwrap()).await;
                        client_manager.unregister_client(&addr).await;
                        info!("Client {} disconnected (RequestUri completed)", addr);
                    }
                }
            }
            Ok(msg) => {
                error!("Unexpected message from {}: {:?}", addr, msg);
                let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                    message: "Expected Intent message".to_string(),
                }).await;
                client_manager.unregister_client(&addr).await;
                info!("Client {} disconnected", addr);
            }
            Err(e) => {
                error!("Failed to read Intent from {}: {}", addr, e);
                client_manager.unregister_client(&addr).await;
                info!("Client {} disconnected", addr);
            }
        }
    }

    /// Handle PrimalLogin intent - authenticate and send server list
    async fn handle_primal_login(mut self, username: String, password_hash: String) {
        debug!("Processing PrimalLogin for user '{}'", username);

        // TODO: Implement actual authentication
        let authenticated = self.authenticate(&username, &password_hash, IntentType::PrimalLogin).await;

        if !authenticated {
            error!("Authentication failed for user '{}'", username);
            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                message: "Authentication failed".to_string(),
            }).await;
            return;
        }

        info!("User '{}' authenticated successfully", username);

        // Get server list
        let server_list = self.get_server_list();

        // Check if list is empty
        if server_list.is_empty() {
            error!("No servers available for user '{}'", username);
            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                message: "No servers available".to_string(),
            }).await;
            return;
        }

        // Send server list
        if let Err(e) = self.stream.write_primal_message(&PrimalMessage::ServerList {
            servers: server_list,
        }).await {
            error!("Failed to send server list to {}: {}", self.addr, e);
        } else {
            debug!("Sent server list to {}", self.addr);
        }
    }

    /// Handle GameLogin intent - authenticate and transition to GameClient
    async fn handle_game_login(mut self, username: String, password_hash: String, game_id: String) {
        debug!("Processing GameLogin for user '{}' on game '{}'", username, game_id);

        // Authenticate with provided credentials
        let authenticated = self.authenticate(&username, &password_hash, IntentType::GameLogin).await;

        if !authenticated {
            error!("Game authentication failed for user '{}'", username);
            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                message: "Unauthorized".to_string(),
            }).await;
            return;
        }

        info!("Game user '{}' authenticated for game '{}', transitioning to GameClient", username, game_id);

        // Create GameClient and hand off the connection
        let game_client = GameClient::new(self.stream, self.addr, username, game_id, Arc::new(self.config.clone()), self.client_manager);
        game_client.handle().await;
    }

    /// Handle RequestUri intent - one-shot URI request for resource download
    async fn handle_request_uri(mut self, username: String, password_hash: String, game_id: String, uri: String) {
        debug!("Processing RequestUri for user '{}' on game '{}': {}", username, game_id, uri);

        // Authenticate with provided credentials
        let authenticated = self.authenticate(&username, &password_hash, IntentType::RequestUri).await;

        if !authenticated {
            error!("RequestUri authentication failed for user '{}'", username);
            let _ = self.stream.write_primal_message(&PrimalMessage::UriResponse {
                status: 401,
                buffer_string: None,
                file_name: None,
                file_size: None,
            }).await;
            return;
        }

        debug!("User '{}' authenticated for RequestUri on game '{}': {}", username, game_id, uri);

        // Get the game runtime for event dispatch
        let response = if let Some(game_runtime) = self.game_runtimes.get(&game_id) {
            // Dispatch to registered RequestUri handlers
            game_runtime.dispatch_request_uri(&uri).await
        } else {
            warn!("No game runtime found for game '{}', returning 404", game_id);
            stam_mod_runtimes::api::UriResponse::default()
        };

        // Adaptive chunk sizing for optimal throughput
        // Start with a larger chunk size for better localhost performance
        const MIN_CHUNK_SIZE: usize = 256 * 1024;     // 256 KB minimum
        const INITIAL_CHUNK_SIZE: usize = 4 * 1024 * 1024;  // Start at 4 MB for fast ramp-up
        const TARGET_CHUNK_TIME_MS: u128 = 50;        // Target ~50ms per chunk for better throughput
        let max_chunk_size = self.config.network_max_chunk_size.as_bytes();

        // Check if we need to read file content
        if !response.filepath.is_empty() {
            // Handler specified a file path - resolve it relative to STAM_HOME
            // and verify it doesn't escape the allowed directory (security check)
            let home_dir = self.game_runtimes.get(&game_id)
                .and_then(|runtime| runtime.get_home_dir());

            let resolved_path: Option<std::path::PathBuf> = if let Some(home) = home_dir {
                // Resolve the path relative to STAM_HOME
                let full_path = home.join(&response.filepath);

                // Canonicalize to resolve any .. or symlinks
                match full_path.canonicalize() {
                    Ok(canonical) => {
                        // Security check: ensure the resolved path is within STAM_HOME
                        match home.canonicalize() {
                            Ok(home_canonical) => {
                                if canonical.starts_with(&home_canonical) {
                                    Some(canonical)
                                } else {
                                    error!("Security violation: filepath '{}' resolves to '{}' which is outside STAM_HOME '{}'",
                                        response.filepath, canonical.display(), home_canonical.display());
                                    None
                                }
                            }
                            Err(e) => {
                                error!("Failed to canonicalize STAM_HOME '{}': {}", home.display(), e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to resolve filepath '{}': {}", full_path.display(), e);
                        None
                    }
                }
            } else {
                error!("No STAM_HOME configured for game '{}', cannot resolve filepath", game_id);
                None
            };

            if let Some(ref path) = resolved_path {
                // Get file metadata
                let file_size = match std::fs::metadata(path) {
                    Ok(meta) => meta.len(),
                    Err(e) => {
                        error!("Failed to get file metadata '{}': {}", path.display(), e);
                        let _ = self.stream.write_primal_message(&PrimalMessage::UriResponse {
                            status: 500,
                            buffer_string: None,
                            file_name: None,
                            file_size: None,
                        }).await;
                        return;
                    }
                };

                // Extract filename
                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string());

                debug!("Sending file '{}' ({} bytes) in chunks for URI '{}'", path.display(), file_size, uri);

                // Send initial UriResponse with metadata (buffer_string is None for chunked transfer)
                if let Err(e) = self.stream.write_primal_message(&PrimalMessage::UriResponse {
                    status: response.status,
                    buffer_string: None,
                    file_name,
                    file_size: Some(file_size),
                }).await {
                    error!("Failed to send UriResponse header: {}", e);
                    return;
                }

                // Stream file content in chunks
                use tokio::io::AsyncReadExt;
                let file = match tokio::fs::File::open(path).await {
                    Ok(f) => f,
                    Err(e) => {
                        error!("Failed to open file '{}': {}", path.display(), e);
                        // Send empty final chunk to signal error
                        let _ = self.stream.write_primal_message(&PrimalMessage::UriResponseChunk {
                            data: Vec::new(),
                            is_final: true,
                        }).await;
                        return;
                    }
                };

                // Use a larger buffer for BufReader to enable bigger reads
                let mut reader = tokio::io::BufReader::with_capacity(max_chunk_size, file);
                let mut current_chunk_size = INITIAL_CHUNK_SIZE.min(max_chunk_size);
                let mut buffer = vec![0u8; max_chunk_size]; // Allocate max size once
                let mut total_sent: u64 = 0;
                let start_time = std::time::Instant::now();

                // Bandwidth limiting configuration
                let bandwidth_limit = self.config.download_bandwidth_limit_x_client_ps.as_bytes();
                let rate_limit_enabled = bandwidth_limit > 0;
                if rate_limit_enabled {
                    debug!("Rate limiting enabled: {} bytes/sec ({}/s)",
                        bandwidth_limit,
                        crate::config::ByteSize(bandwidth_limit));
                }

                loop {
                    // Read exactly current_chunk_size bytes (or less if EOF)
                    // We use read_buf pattern to fill as much as possible
                    let mut bytes_read = 0;
                    while bytes_read < current_chunk_size {
                        match reader.read(&mut buffer[bytes_read..current_chunk_size]).await {
                            Ok(0) => break, // EOF
                            Ok(n) => bytes_read += n,
                            Err(e) => {
                                error!("Failed to read file '{}': {}", path.display(), e);
                                // Send empty final chunk to signal error
                                let _ = self.stream.write_primal_message(&PrimalMessage::UriResponseChunk {
                                    data: Vec::new(),
                                    is_final: true,
                                }).await;
                                return;
                            }
                        }
                    }

                    if bytes_read == 0 {
                        break; // EOF reached
                    }

                    total_sent += bytes_read as u64;
                    let is_final = total_sent >= file_size;

                    // Measure time to send this chunk
                    let chunk_start = std::time::Instant::now();

                    // Use raw chunk writing to avoid allocations
                    if let Err(e) = self.stream.write_raw_chunk(&buffer[..bytes_read], is_final).await {
                        error!("Failed to send file chunk: {}", e);
                        return;
                    }

                    let chunk_elapsed_ms = chunk_start.elapsed().as_millis();

                    // Log current chunk size in human-readable format
                    trace!("Sent chunk: {} ({} bytes) in {}ms",
                        crate::config::ByteSize(bytes_read).to_string(),
                        bytes_read,
                        chunk_elapsed_ms);

                    // Apply bandwidth limiting if configured
                    if rate_limit_enabled {
                        // Calculate how long this chunk should take to send at the limited rate
                        let expected_duration_ms = (bytes_read as f64 / bandwidth_limit as f64 * 1000.0) as u64;
                        let actual_duration_ms = chunk_elapsed_ms as u64;

                        // If we sent too fast, sleep to enforce the rate limit
                        if actual_duration_ms < expected_duration_ms {
                            let sleep_duration_ms = expected_duration_ms - actual_duration_ms;
                            trace!("Rate limiting: sleeping {}ms (chunk sent in {}ms, should take {}ms at {}/s)",
                                sleep_duration_ms,
                                actual_duration_ms,
                                expected_duration_ms,
                                crate::config::ByteSize(bandwidth_limit));
                            tokio::time::sleep(tokio::time::Duration::from_millis(sleep_duration_ms)).await;
                        }
                    }

                    // Adapt chunk size based on transfer speed
                    // Goal: maximize throughput while keeping chunks responsive
                    // Only increase, never decrease - TCP flow control handles congestion
                    // When rate limiting is enabled, adaptive chunk sizing is less relevant
                    if !rate_limit_enabled && chunk_elapsed_ms > 0 && chunk_elapsed_ms < TARGET_CHUNK_TIME_MS {
                        // Below target time: aggressively increase chunk size
                        let multiplier = if chunk_elapsed_ms < TARGET_CHUNK_TIME_MS / 4 {
                            4  // Very fast (<12ms): quadruple
                        } else if chunk_elapsed_ms < TARGET_CHUNK_TIME_MS / 2 {
                            2  // Fast (<25ms): double
                        } else {
                            3  // Moderate (<50ms): increase by 50% (3/2)
                        };
                        if multiplier == 3 {
                            current_chunk_size = (current_chunk_size * 3 / 2).min(max_chunk_size);
                        } else {
                            current_chunk_size = (current_chunk_size * multiplier).min(max_chunk_size);
                        }
                    }
                    // Note: We don't decrease chunk size anymore - TCP backpressure naturally
                    // limits throughput, and smaller chunks have more overhead

                    if is_final {
                        break;
                    }
                }

                let total_elapsed = start_time.elapsed();
                let speed_mbps = if total_elapsed.as_secs_f64() > 0.0 {
                    (total_sent as f64 / 1024.0 / 1024.0) / total_elapsed.as_secs_f64()
                } else {
                    0.0
                };
                debug!("Finished sending file '{}' ({} bytes in {:?}, {:.2} MB/s, final chunk size: {} KB)",
                    path.display(), total_sent, total_elapsed, speed_mbps, current_chunk_size / 1024);
            } else {
                // Path resolution failed
                let _ = self.stream.write_primal_message(&PrimalMessage::UriResponse {
                    status: 404,
                    buffer_string: None,
                    file_name: None,
                    file_size: None,
                }).await;
            }
        } else if !response.buffer_string.is_empty() {
            // Handler provided buffer string directly
            debug!("RequestUri response: status={}, buffer_string length={}", response.status, response.buffer_string.len());

            let _ = self.stream.write_primal_message(&PrimalMessage::UriResponse {
                status: response.status,
                buffer_string: Some(response.buffer_string),
                file_name: None,
                file_size: None,
            }).await;
        } else {
            // No content
            debug!("RequestUri response: status={}, no content", response.status);

            let _ = self.stream.write_primal_message(&PrimalMessage::UriResponse {
                status: response.status,
                buffer_string: None,
                file_name: None,
                file_size: None,
            }).await;
        }
    }

    /// Authenticate user credentials based on intent type
    /// TODO: Implement actual authentication logic with different rules per intent
    async fn authenticate(&self, _username: &str, _password_hash: &str, _intent: IntentType) -> bool {
        // For now, always return true
        // In the future, this can:
        // - Check different user databases based on intent
        // - Apply different permission levels (PrimalLogin vs GameLogin)
        // - Enforce rate limits or IP restrictions per intent type
        // - Log authentication attempts differently
        true
    }

    /// Get list of available game servers from configuration
    /// Returns one ServerInfo for each game in the configuration
    /// Returns empty list if public_uri is not configured or no games available
    fn get_server_list(&self) -> Vec<ServerInfo> {
        if let Some(uri) = &self.config.public_uri {
            // Create a ServerInfo for each configured game
            self.config.games.iter().map(|(game_id, game_config)| {
                ServerInfo {
                    game_id: game_id.clone(),
                    name: game_config.name.clone(),
                    uri: uri.clone(),
                }
            }).collect()
        } else {
            // No public_uri configured, return empty list
            Vec::new()
        }
    }

    /// Check if client version is compatible with server version
    /// Returns true if major.minor versions match
    fn is_version_compatible(&self, client_version: &str) -> bool {
        let server_major_minor = self.get_major_minor(VERSION);
        let client_major_minor = self.get_major_minor(client_version);

        server_major_minor == client_major_minor
    }

    /// Extract major.minor from a version string (e.g., "0.1.0-alpha" -> "0.1")
    fn get_major_minor(&self, version: &str) -> String {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() >= 2 {
            format!("{}.{}", parts[0], parts[1])
        } else {
            version.to_string()
        }
    }
}
