use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::{info, debug, error, warn};

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
                buffer: None,
                file_name: None,
                file_size: None,
            }).await;
            return;
        }

        info!("User '{}' authenticated for RequestUri on game '{}': {}", username, game_id, uri);

        // Get the game runtime for event dispatch
        let response = if let Some(game_runtime) = self.game_runtimes.get(&game_id) {
            // Dispatch to registered RequestUri handlers
            game_runtime.dispatch_request_uri(&uri).await
        } else {
            warn!("No game runtime found for game '{}', returning 404", game_id);
            stam_mod_runtimes::api::UriResponse::default()
        };

        // Check if we need to read file content
        let (buffer, file_size, resolved_filepath) = if !response.filepath.is_empty() {
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
                match std::fs::read(path) {
                    Ok(content) => {
                        let size = content.len() as u64;
                        // Log file transfers at INFO level, especially for mod downloads (ZIP files)
                        if path.to_string_lossy().ends_with(".zip") {
                            info!("Sending ZIP file '{}' ({} bytes) to user '{}' for URI '{}'",
                                path.display(), size, username, uri);
                        } else {
                            debug!("Sending file '{}' ({} bytes) for URI '{}'", path.display(), size, uri);
                        }
                        (Some(content), Some(size), resolved_path)
                    }
                    Err(e) => {
                        error!("Failed to read file '{}': {}", path.display(), e);
                        (None, None, None)
                    }
                }
            } else {
                (None, None, None)
            }
        } else if !response.buffer.is_empty() {
            // Handler provided buffer directly
            // Use buffer_size from response to truncate buffer (optimization for network transfer)
            let effective_size = if response.buffer_size > 0 {
                (response.buffer_size as usize).min(response.buffer.len())
            } else {
                response.buffer.len()
            };
            let truncated_buffer = response.buffer[..effective_size].to_vec();
            (Some(truncated_buffer), Some(effective_size as u64), None)
        } else {
            (None, None, None)
        };

        // Extract filename from resolved filepath if present
        let file_name = resolved_filepath.as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());

        debug!("RequestUri response: status={}, file_name={:?}, file_size={:?}", response.status, file_name, file_size);

        let _ = self.stream.write_primal_message(&PrimalMessage::UriResponse {
            status: response.status,
            buffer,
            file_name,
            file_size,
        }).await;
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
