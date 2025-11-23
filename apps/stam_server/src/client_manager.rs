use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::TcpStream;
use tracing::info;
use stam_protocol::{PrimalMessage, GameMessage};
use stam_protocol::stream::{PrimalStream, GameStream};

/// Type of client connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientType {
    Primal,
    Game,
    Server,
}

/// Client connection handle
#[derive(Debug)]
pub struct ClientHandle {
    pub addr: SocketAddr,
    pub client_type: ClientType,
    pub username: Option<String>,
}

/// Manager for tracking active client connections
#[derive(Clone)]
pub struct ClientManager {
    /// Map of client address to client handle
    clients: Arc<RwLock<HashMap<SocketAddr, ClientHandle>>>,
}

impl ClientManager {
    /// Create a new client manager
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new client connection
    pub async fn register_client(&self, addr: SocketAddr, client_type: ClientType, username: Option<String>) {
        let mut clients = self.clients.write().await;
        let handle = ClientHandle {
            addr,
            client_type,
            username: username.clone(),
        };
        clients.insert(addr, handle);

        let total = clients.len();
        let user_info = username.as_ref().map(|u| format!(" ({})", u)).unwrap_or_default();
        info!("Registered {:?} client from {}{} - Total active: {}", client_type, addr, user_info, total);
    }

    /// Unregister a client connection
    pub async fn unregister_client(&self, addr: &SocketAddr) {
        let mut clients = self.clients.write().await;
        if let Some(handle) = clients.remove(addr) {
            let total = clients.len();
            let user_info = handle.username.as_ref().map(|u| format!(" ({})", u)).unwrap_or_default();
            info!("Unregistered {:?} client from {}{} - Total active: {}", handle.client_type, addr, user_info, total);
        }
    }

    /// Get count of active clients by type
    pub async fn get_client_count(&self, client_type: ClientType) -> usize {
        let clients = self.clients.read().await;
        clients.values().filter(|h| h.client_type == client_type).count()
    }

    /// Get total count of active clients
    pub async fn get_total_count(&self) -> usize {
        let clients = self.clients.read().await;
        clients.len()
    }

    /// Disconnect all clients with a custom message
    pub async fn disconnect_all(&self, message: &str) {
        let clients = self.clients.read().await;
        let count = clients.len();

        if count == 0 {
            return;
        }

        info!("Disconnecting {} clients: {}", count, message);

        // Disconnect each client based on type
        for (addr, handle) in clients.iter() {
            match handle.client_type {
                ClientType::Primal => {
                    // For Primal clients, try to reconnect and send disconnect message
                    if let Ok(stream) = TcpStream::connect(addr).await {
                        let mut stream = stream;
                        let _ = stream.write_primal_message(&PrimalMessage::Disconnect {
                            message: message.to_string(),
                        }).await;
                    }
                }
                ClientType::Game => {
                    // For Game clients, try to reconnect and send disconnect message
                    if let Ok(stream) = TcpStream::connect(addr).await {
                        let mut stream = stream;
                        let _ = stream.write_game_message(&GameMessage::Disconnect {
                            message: message.to_string(),
                        }).await;
                    }
                }
                ClientType::Server => {
                    // Server-to-server disconnect logic (future implementation)
                }
            }
        }

        info!("All clients disconnected");
    }

    /// Get list of client addresses by type
    pub async fn get_clients_by_type(&self, client_type: ClientType) -> Vec<SocketAddr> {
        let clients = self.clients.read().await;
        clients.values()
            .filter(|h| h.client_type == client_type)
            .map(|h| h.addr)
            .collect()
    }
}
