//! GraphicProxy - Central coordinator for graphic operations
//!
//! The GraphicProxy provides a language-agnostic API for graphic operations,
//! routing commands to the active graphic engine via message channels.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use super::{GraphicCommand, GraphicEngineInfo, GraphicEngines, GraphicEvent, InitialWindowConfig, WindowConfig, WindowInfo};

/// Request to enable a graphic engine
///
/// Sent from the worker thread to the main thread when a mod calls
/// `graphic.enableEngine()`. The main thread then spawns the appropriate
/// engine and sends back the channels for communication.
pub struct EnableEngineRequest {
    /// Which engine to enable
    pub engine_type: GraphicEngines,
    /// Initial window configuration (applied at engine startup)
    pub initial_window_config: Option<InitialWindowConfig>,
    /// Channel to send back the command sender and event receiver
    pub response_tx: oneshot::Sender<
        Result<
            (
                std::sync::mpsc::Sender<GraphicCommand>,
                mpsc::Receiver<GraphicEvent>,
            ),
            String,
        >,
    >,
}

/// Central proxy for graphic engine operations
///
/// This struct is shared across all mod contexts and ALL scripting runtimes
/// (JavaScript, Lua, C#, etc.). It provides a unified interface for graphic
/// operations regardless of the underlying engine or calling language.
///
/// # Thread Safety
///
/// GraphicProxy is designed to be shared across threads using `Arc`.
/// Internal state is protected by `RwLock` and `Mutex` as appropriate.
///
/// # Client vs Server
///
/// On the client, `available` is true and all operations work normally.
/// On the server, `available` is false and all operations return descriptive errors.
pub struct GraphicProxy {
    /// Currently active engine type (None if no engine enabled)
    active_engine: Arc<RwLock<Option<GraphicEngines>>>,

    /// Channel to send commands to the graphic engine thread
    command_tx: Arc<RwLock<Option<std::sync::mpsc::Sender<GraphicCommand>>>>,

    /// Channel to request engine enablement from main thread
    enable_request_tx: Arc<RwLock<Option<std::sync::mpsc::Sender<EnableEngineRequest>>>>,

    /// Event receiver from the graphic engine (stored for the main loop to poll)
    event_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<GraphicEvent>>>>,

    /// Window registry - maps window IDs to their state
    windows: Arc<RwLock<HashMap<u64, WindowInfo>>>,

    /// Next window ID counter
    next_window_id: AtomicU64,

    /// Flag: graphic proxy is available (client-only)
    available: bool,
}

impl GraphicProxy {
    /// Create a new GraphicProxy for the client
    ///
    /// The `enable_request_tx` channel is used to request engine enablement
    /// from the main thread.
    pub fn new_client(
        enable_request_tx: std::sync::mpsc::Sender<EnableEngineRequest>,
    ) -> Self {
        Self {
            active_engine: Arc::new(RwLock::new(None)),
            command_tx: Arc::new(RwLock::new(None)),
            enable_request_tx: Arc::new(RwLock::new(Some(enable_request_tx))),
            event_rx: Arc::new(tokio::sync::Mutex::new(None)),
            windows: Arc::new(RwLock::new(HashMap::new())),
            next_window_id: AtomicU64::new(1),
            available: true,
        }
    }

    /// Create a stub GraphicProxy for the server
    ///
    /// All operations will return errors indicating they are client-only.
    pub fn new_server_stub() -> Self {
        Self {
            active_engine: Arc::new(RwLock::new(None)),
            command_tx: Arc::new(RwLock::new(None)),
            enable_request_tx: Arc::new(RwLock::new(None)),
            event_rx: Arc::new(tokio::sync::Mutex::new(None)),
            windows: Arc::new(RwLock::new(HashMap::new())),
            next_window_id: AtomicU64::new(1),
            available: false,
        }
    }

    /// Check if the graphic proxy is available
    ///
    /// Returns false on server, true on client.
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Check if a graphic engine is currently enabled
    pub fn is_engine_enabled(&self) -> bool {
        self.active_engine.read().unwrap().is_some()
    }

    /// Get the currently active engine type
    pub fn get_active_engine(&self) -> Option<GraphicEngines> {
        *self.active_engine.read().unwrap()
    }

    /// Get information about the active graphic engine
    ///
    /// Returns detailed information about the currently enabled engine,
    /// including version, capabilities, and rendering backend.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Called on the server
    /// - No graphic engine is enabled
    /// - The engine fails to respond
    pub async fn get_engine_info(&self) -> Result<GraphicEngineInfo, String> {
        if !self.available {
            return Err(
                "graphic.getEngineInfo() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call graphic.enableEngine() first.")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::GetEngineInfo { response_tx })
            .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond".to_string())
    }

    /// Enable a graphic engine
    ///
    /// This sends a request to the main thread to spawn the engine.
    /// The main thread will create the engine and send back the channels
    /// for communication.
    ///
    /// # Arguments
    /// * `engine_type` - The type of engine to enable
    /// * `initial_window_config` - Optional configuration for the main window
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Called on the server
    /// - An engine is already enabled
    /// - The engine type is not supported
    /// - The main thread fails to spawn the engine
    pub async fn enable_engine(
        &self,
        engine_type: GraphicEngines,
        initial_window_config: Option<InitialWindowConfig>,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "graphic.enableEngine() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        if self.is_engine_enabled() {
            return Err("A graphic engine is already enabled".to_string());
        }

        if !engine_type.is_supported() {
            return Err(format!(
                "Graphic engine '{}' is not yet supported",
                engine_type.name()
            ));
        }

        // Get the enable request channel
        let enable_tx = {
            let guard = self.enable_request_tx.read().unwrap();
            guard.clone().ok_or("Enable request channel not available")?
        };

        // Create response channel
        let (response_tx, response_rx) = oneshot::channel();

        // Send enable request to main thread
        enable_tx
            .send(EnableEngineRequest {
                engine_type,
                initial_window_config,
                response_tx,
            })
            .map_err(|_| "Failed to send enable request to main thread")?;

        // Wait for response
        let (cmd_tx, event_rx) = response_rx
            .await
            .map_err(|_| "Main thread did not respond to enable request")??;

        // Store the command sender
        *self.command_tx.write().unwrap() = Some(cmd_tx);

        // Store the event receiver for polling by the main loop
        *self.event_rx.lock().await = Some(event_rx);

        // Set active engine
        *self.active_engine.write().unwrap() = Some(engine_type);

        tracing::info!("Graphic engine '{}' enabled", engine_type.name());

        Ok(())
    }

    /// Create a new window
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Called on the server
    /// - No graphic engine is enabled
    /// - The engine fails to create the window
    pub async fn create_window(&self, config: WindowConfig) -> Result<u64, String> {
        if !self.available {
            return Err(
                "graphic.createWindow() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call graphic.enableEngine() first.")?;

        let window_id = self.next_window_id.fetch_add(1, Ordering::SeqCst);
        let (response_tx, response_rx) = oneshot::channel();

        tracing::debug!("Sending CreateWindow command for window {}", window_id);

        tx.send(GraphicCommand::CreateWindow {
            id: window_id,
            config: config.clone(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        tracing::debug!("Waiting for CreateWindow response for window {}", window_id);

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Track window
        let mut window_info = WindowInfo::new(window_id, config);
        window_info.mark_created();
        self.windows.write().unwrap().insert(window_id, window_info);

        tracing::debug!("Window {} created", window_id);

        Ok(window_id)
    }

    /// Close a window
    pub async fn close_window(&self, window_id: u64) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.close() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::CloseWindow {
            id: window_id,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Remove from tracking
        self.windows.write().unwrap().remove(&window_id);

        tracing::debug!("Window {} closed", window_id);

        Ok(())
    }

    /// Set window size
    pub async fn set_window_size(
        &self,
        window_id: u64,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.setSize() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SetWindowSize {
            id: window_id,
            width,
            height,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update tracking
        if let Some(info) = self.windows.write().unwrap().get_mut(&window_id) {
            info.config.width = width;
            info.config.height = height;
        }

        Ok(())
    }

    /// Set window title
    pub async fn set_window_title(&self, window_id: u64, title: String) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.setTitle() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SetWindowTitle {
            id: window_id,
            title: title.clone(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update tracking
        if let Some(info) = self.windows.write().unwrap().get_mut(&window_id) {
            info.config.title = title;
        }

        Ok(())
    }

    /// Set window fullscreen mode
    pub async fn set_window_fullscreen(
        &self,
        window_id: u64,
        fullscreen: bool,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.setFullscreen() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SetWindowFullscreen {
            id: window_id,
            fullscreen,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update tracking
        if let Some(info) = self.windows.write().unwrap().get_mut(&window_id) {
            info.config.fullscreen = fullscreen;
        }

        Ok(())
    }

    /// Set window visibility
    pub async fn set_window_visible(&self, window_id: u64, visible: bool) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.setVisible() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SetWindowVisible {
            id: window_id,
            visible,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update tracking
        if let Some(info) = self.windows.write().unwrap().get_mut(&window_id) {
            info.config.visible = visible;
        }

        Ok(())
    }

    // Note: set_window_resizable was removed - resizable must be set at window creation time

    /// Shutdown the graphic engine gracefully
    ///
    /// Sends a shutdown command to the engine and waits for it to complete
    /// within the specified timeout.
    ///
    /// # Arguments
    /// * `timeout` - Maximum time to wait for shutdown
    ///
    /// # Returns
    /// * `Ok(())` - Engine shut down successfully or was not running
    /// * `Err(String)` - Shutdown timed out or failed
    pub async fn shutdown(&self, timeout: Duration) -> Result<(), String> {
        if let Some(tx) = self.command_tx.read().unwrap().as_ref() {
            let (response_tx, response_rx) = oneshot::channel();

            if tx.send(GraphicCommand::Shutdown { response_tx }).is_err() {
                // Channel closed, engine already dead
                return Ok(());
            }

            match tokio::time::timeout(timeout, response_rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Ok(()), // Channel closed, engine exited
                Err(_) => Err("Graphic engine shutdown timed out".to_string()),
            }
        } else {
            Ok(()) // No engine running
        }
    }

    /// Get information about a window
    pub fn get_window_info(&self, window_id: u64) -> Option<WindowInfo> {
        self.windows.read().unwrap().get(&window_id).cloned()
    }

    /// Get all window IDs
    pub fn get_window_ids(&self) -> Vec<u64> {
        self.windows.read().unwrap().keys().copied().collect()
    }

    /// Take the event receiver for polling by the main event loop
    ///
    /// This method takes ownership of the event receiver, so it can only be called once.
    /// The caller is responsible for polling events and dispatching them to mods.
    ///
    /// Returns None if:
    /// - No engine has been enabled yet
    /// - The receiver has already been taken
    /// - Running on the server (where graphic is not available)
    pub async fn take_event_receiver(&self) -> Option<mpsc::Receiver<GraphicEvent>> {
        self.event_rx.lock().await.take()
    }

    /// Try to receive an event without blocking
    ///
    /// This is a convenience method for polling events in a loop.
    /// Returns None if no events are available or the receiver was already taken.
    pub async fn try_recv_event(&self) -> Option<GraphicEvent> {
        let mut guard = self.event_rx.lock().await;
        if let Some(ref mut rx) = *guard {
            rx.try_recv().ok()
        } else {
            None
        }
    }
}

// GraphicProxy is Send + Sync because all internal state is protected
unsafe impl Send for GraphicProxy {}
unsafe impl Sync for GraphicProxy {}
