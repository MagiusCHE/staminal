use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::info;

/// Type of client connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientType {
    Primal,
    Game,
    Server,
}

/// Commands that can be sent to client handlers
#[derive(Debug, Clone)]
pub enum ClientCommand {
    /// Disconnect with a message ID
    Disconnect { message_id: String },
}

/// Client connection handle
#[derive(Debug)]
pub struct ClientHandle {
    pub addr: SocketAddr,
    pub client_type: ClientType,
    pub username: Option<String>,
    /// Channel to send commands to this client's handler
    pub command_tx: mpsc::UnboundedSender<ClientCommand>,
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
    /// Returns a receiver for commands that should be handled by the client handler
    pub async fn register_client(&self, addr: SocketAddr, client_type: ClientType, username: Option<String>) -> mpsc::UnboundedReceiver<ClientCommand> {
        let mut clients = self.clients.write().await;
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        let handle = ClientHandle {
            addr,
            client_type,
            username: username.clone(),
            command_tx,
        };
        clients.insert(addr, handle);

        let total = clients.len();
        let user_info = username.as_ref().map(|u| format!(" ({})", u)).unwrap_or_default();
        info!("Registered {:?} client from {}{} - Total active: {}", client_type, addr, user_info, total);

        command_rx
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

    /// Disconnect all clients with a message ID
    pub async fn disconnect_all(&self, message_id: &str) {
        let clients = self.clients.read().await;
        let count = clients.len();

        if count == 0 {
            return;
        }

        info!("Sending disconnect command to {} clients with message ID: {}", count, message_id);

        // Send disconnect command to all client handlers
        for (addr, handle) in clients.iter() {
            let command = ClientCommand::Disconnect {
                message_id: message_id.to_string(),
            };

            if let Err(e) = handle.command_tx.send(command) {
                info!("Failed to send disconnect command to {}: {}", addr, e);
            }
        }

        info!("Disconnect commands sent to all clients");
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
