use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{info, debug, error, warn};

use stam_protocol::{IntentType, PrimalMessage, PrimalStream, ServerInfo};

use crate::game_client::GameClient;
use crate::config::Config;
use crate::client_manager::{ClientManager, ClientType};
use crate::VERSION;

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
}

impl PrimalClient {
    /// Create a new PrimalClient from an accepted TCP connection
    pub fn new(stream: TcpStream, addr: SocketAddr, config: Config, client_manager: ClientManager) -> Self {
        info!("New client connected from {}", addr);
        Self { stream, addr, config, client_manager }
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
        client_manager.register_client(addr, ClientType::Primal, None).await;

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
            Ok(PrimalMessage::Intent { intent_type, username, password_hash }) => {
                debug!("Received Intent from {}: {:?}, user={}", addr, intent_type, username);

                match intent_type {
                    IntentType::PrimalLogin => {
                        self.handle_primal_login(username, password_hash).await;
                        // Unregister after PrimalLogin completes
                        client_manager.unregister_client(&addr).await;
                        info!("Client {} disconnected", addr);
                    }
                    IntentType::GameLogin => {
                        // Unregister as Primal before transitioning to Game
                        client_manager.unregister_client(&addr).await;
                        self.handle_game_login(username, password_hash).await;
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
    async fn handle_game_login(mut self, username: String, password_hash: String) {
        debug!("Processing GameLogin for user '{}'", username);

        // Authenticate with provided credentials
        let authenticated = self.authenticate(&username, &password_hash, IntentType::GameLogin).await;

        if !authenticated {
            error!("Game authentication failed for user '{}'", username);
            let _ = self.stream.write_primal_message(&PrimalMessage::Error {
                message: "Unauthorized".to_string(),
            }).await;
            return;
        }

        info!("Game user '{}' authenticated, transitioning to GameClient", username);

        // Create GameClient and hand off the connection
        let game_client = GameClient::new(self.stream, self.addr, username, self.client_manager);
        game_client.handle().await;
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
    /// Returns empty list if public_uri is not configured
    fn get_server_list(&self) -> Vec<ServerInfo> {
        if let Some(uri) = &self.config.public_uri {
            vec![ServerInfo {
                name: self.config.name.clone(),
                uri: uri.clone(),
            }]
        } else {
            // No public_uri configured, return empty list
            Vec::new()
        }
    }
}
