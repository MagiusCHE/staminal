use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{info, debug, error};

use stam_protocol::{GameMessage, GameStream};
use crate::client_manager::{ClientManager, ClientType};

/// GameClient represents an authenticated game client connection
/// Handles game-specific protocol messages
pub struct GameClient {
    /// TCP stream for this client connection
    stream: TcpStream,
    /// Remote address of the client
    addr: SocketAddr,
    /// Username after authentication
    username: String,
    /// Client manager for tracking connections
    client_manager: ClientManager,
}

impl GameClient {
    /// Create a new GameClient from an authenticated connection
    pub fn new(stream: TcpStream, addr: SocketAddr, username: String, client_manager: ClientManager) -> Self {
        info!("Game client created for user '{}' from {}", username, addr);
        Self { stream, addr, username, client_manager }
    }

    /// Get the client's remote address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the client's username
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Handle the game client connection
    /// Client is already authenticated, send LoginSuccess and maintain connection
    pub async fn handle(mut self) {
        let addr = self.addr;
        let username = self.username.clone();

        // Register as Game client
        self.client_manager.register_client(addr, ClientType::Game, Some(username.clone())).await;

        debug!("Handling authenticated game client from {}", addr);

        // Send LoginSuccess immediately (already authenticated via Intent)
        if let Err(e) = self.stream.write_game_message(&GameMessage::LoginSuccess).await {
            error!("Failed to send LoginSuccess to {}: {}", addr, e);
            self.client_manager.unregister_client(&addr).await;
            return;
        }

        info!("Sent LoginSuccess to user '{}'", username);

        // Keep connection alive - wait for game messages
        self.maintain_connection().await;

        // Unregister when connection ends
        self.client_manager.unregister_client(&addr).await;
        info!("Game client {} disconnected", addr);
    }

    /// Maintain the connection alive until client disconnects or server shuts down
    async fn maintain_connection(&mut self) {
        debug!("Maintaining connection for {}", self.addr);

        loop {
            match self.stream.read_game_message().await {
                Ok(msg) => {
                    debug!("Received message from {}: {:?}", self.addr, msg);
                    // TODO: Handle game messages
                }
                Err(e) => {
                    debug!("Connection closed for {}: {}", self.addr, e);
                    break;
                }
            }
        }
    }
}
