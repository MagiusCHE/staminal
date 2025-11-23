use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{info, debug, error};

use stam_protocol::{GameMessage, GameStream};
use crate::client_manager::{ClientManager, ClientType, ClientCommand};

/// GameClient represents an authenticated game client connection
/// Handles game-specific protocol messages
pub struct GameClient {
    /// TCP stream for this client connection
    stream: TcpStream,
    /// Remote address of the client
    addr: SocketAddr,
    /// Username after authentication
    username: String,
    /// Game ID the client is connected to
    game_id: String,
    /// Client manager for tracking connections
    client_manager: ClientManager,
}

impl GameClient {
    /// Create a new GameClient from an authenticated connection
    pub fn new(stream: TcpStream, addr: SocketAddr, username: String, game_id: String, client_manager: ClientManager) -> Self {
        info!("Game client created for user '{}' on game '{}' from {}", username, game_id, addr);
        Self { stream, addr, username, game_id, client_manager }
    }

    /// Get the client's remote address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the client's username
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Get the game ID
    pub fn game_id(&self) -> &str {
        &self.game_id
    }

    /// Handle the game client connection
    /// Client is already authenticated, send LoginSuccess and maintain connection
    pub async fn handle(mut self) {
        let addr = self.addr;
        let username = self.username.clone();

        // Register as Game client and get command receiver
        let mut command_rx = self.client_manager.register_client(addr, ClientType::Game, Some(username.clone())).await;

        debug!("Handling authenticated game client from {}", addr);

        // Send LoginSuccess immediately (already authenticated via Intent)
        if let Err(e) = self.stream.write_game_message(&GameMessage::LoginSuccess).await {
            error!("Failed to send LoginSuccess to {}: {}", addr, e);
            self.client_manager.unregister_client(&addr).await;
            return;
        }

        info!("Sent LoginSuccess to user '{}'", username);

        // Keep connection alive - wait for game messages or commands
        self.maintain_connection(&mut command_rx).await;

        // Unregister when connection ends
        self.client_manager.unregister_client(&addr).await;
        info!("Game client {} disconnected", addr);
    }

    /// Maintain the connection alive until client disconnects or server shuts down
    async fn maintain_connection(&mut self, command_rx: &mut mpsc::UnboundedReceiver<ClientCommand>) {
        debug!("Maintaining connection for {}", self.addr);

        loop {
            tokio::select! {
                // Handle incoming game messages from client
                msg_result = self.stream.read_game_message() => {
                    match msg_result {
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
                // Handle commands from server (e.g., disconnect)
                Some(command) = command_rx.recv() => {
                    match command {
                        ClientCommand::Disconnect { message_id } => {
                            info!("Sending disconnect message to {}: {}", self.addr, message_id);
                            if let Err(e) = self.stream.write_game_message(&GameMessage::Disconnect {
                                message: message_id,
                            }).await {
                                error!("Failed to send disconnect to {}: {}", self.addr, e);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}
