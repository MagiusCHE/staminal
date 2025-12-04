//! GraphicProxy - Central coordinator for graphic operations
//!
//! The GraphicProxy provides a language-agnostic API for graphic operations,
//! routing commands to the active graphic engine via message channels.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use super::{
    FontInfo, GraphicCommand, GraphicEngineInfo, GraphicEngines, GraphicEvent,
    InitialWindowConfig, PropertyValue, WidgetConfig, WidgetEventType, WidgetInfo,
    WidgetSubscriptions, WidgetType, WindowConfig, WindowInfo, WindowMode,
};
use super::super::path_security::{PathSecurityConfig, validate_and_resolve_path};

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
    /// Root directory for loading assets (e.g., data_dir containing mods)
    pub asset_root: Option<std::path::PathBuf>,
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

    /// Widget registry - maps widget IDs to their info
    widgets: Arc<RwLock<HashMap<u64, WidgetInfo>>>,

    /// Next widget ID counter
    next_widget_id: AtomicU64,

    /// Widget event subscriptions
    widget_subscriptions: Arc<RwLock<WidgetSubscriptions>>,

    /// Loaded fonts (alias -> FontInfo)
    loaded_fonts: Arc<RwLock<HashMap<String, FontInfo>>>,

    /// Flag: graphic proxy is available (client-only)
    available: bool,

    /// Root directory for loading assets (e.g., data_dir containing mods)
    asset_root: Option<std::path::PathBuf>,
}

impl GraphicProxy {
    /// Create a new GraphicProxy for the client
    ///
    /// # Arguments
    /// * `enable_request_tx` - Channel to request engine enablement from the main thread
    /// * `asset_root` - Root directory for loading assets (e.g., data_dir containing mods)
    pub fn new_client(
        enable_request_tx: std::sync::mpsc::Sender<EnableEngineRequest>,
        asset_root: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            active_engine: Arc::new(RwLock::new(None)),
            command_tx: Arc::new(RwLock::new(None)),
            enable_request_tx: Arc::new(RwLock::new(Some(enable_request_tx))),
            event_rx: Arc::new(tokio::sync::Mutex::new(None)),
            windows: Arc::new(RwLock::new(HashMap::new())),
            next_window_id: AtomicU64::new(2), // Start from 2, ID 1 is reserved for main window
            widgets: Arc::new(RwLock::new(HashMap::new())),
            next_widget_id: AtomicU64::new(1),
            widget_subscriptions: Arc::new(RwLock::new(WidgetSubscriptions::new())),
            loaded_fonts: Arc::new(RwLock::new(HashMap::new())),
            available: true,
            asset_root,
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
            next_window_id: AtomicU64::new(2), // Start from 2, ID 1 is reserved for main window
            widgets: Arc::new(RwLock::new(HashMap::new())),
            next_widget_id: AtomicU64::new(1),
            widget_subscriptions: Arc::new(RwLock::new(WidgetSubscriptions::new())),
            loaded_fonts: Arc::new(RwLock::new(HashMap::new())),
            available: false,
            asset_root: None,
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

        // Clone for local use after sending to main thread
        let initial_window_config_clone = initial_window_config.clone();

        // Send enable request to main thread
        enable_tx
            .send(EnableEngineRequest {
                engine_type,
                initial_window_config,
                asset_root: self.asset_root.clone(),
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

        // Register main window (ID 1) in the cache
        let main_window_config: WindowConfig = initial_window_config_clone
            .unwrap_or_default()
            .into();
        let mut main_window_info = WindowInfo::new(1, main_window_config);
        main_window_info.mark_created();
        self.windows.write().unwrap().insert(1, main_window_info);

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

        // tracing::debug!("Waiting for CreateWindow response for window {}", window_id);

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Track window
        let mut window_info = WindowInfo::new(window_id, config);
        window_info.mark_created();
        self.windows.write().unwrap().insert(window_id, window_info);

        // tracing::debug!("Window {} created", window_id);

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

    /// Set window mode (windowed, fullscreen, borderless fullscreen)
    pub async fn set_window_mode(
        &self,
        window_id: u64,
        mode: WindowMode,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.setMode() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SetWindowMode {
            id: window_id,
            mode,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update tracking
        if let Some(info) = self.windows.write().unwrap().get_mut(&window_id) {
            info.config.fullscreen = mode == WindowMode::Fullscreen || mode == WindowMode::BorderlessFullscreen;
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

    /// Set the default font for a window
    ///
    /// All widgets in this window will inherit this font configuration
    /// unless they override it with their own font settings.
    ///
    /// # Arguments
    /// * `window_id` - The window to set the font for
    /// * `family` - Font family alias (must be loaded via graphic.loadFont())
    /// * `size` - Font size in pixels
    pub async fn set_window_font(
        &self,
        window_id: u64,
        family: String,
        size: f32,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.setFont() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SetWindowFont {
            id: window_id,
            family,
            size,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

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

    // ========================================================================
    // Widget Creation and Modification
    // ========================================================================

    /// Create a new widget in a window
    ///
    /// # Arguments
    /// * `window_id` - The window to create the widget in
    /// * `widget_type` - The type of widget to create
    /// * `config` - Widget configuration
    ///
    /// # Returns
    /// * `Ok(widget_id)` - The ID of the created widget
    /// * `Err(String)` - Error message if creation failed
    pub async fn create_widget(
        &self,
        window_id: u64,
        widget_type: WidgetType,
        config: WidgetConfig,
    ) -> Result<u64, String> {
        if !self.available {
            return Err(
                "window.createWidget() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call graphic.enableEngine() first.")?;

        let widget_id = self.next_widget_id.fetch_add(1, Ordering::SeqCst);
        let parent_id = config.parent_id;
        let (response_tx, response_rx) = oneshot::channel();

        // tracing::debug!(
        //     "Creating widget {} (type: {:?}) in window {}",
        //     widget_id,
        //     widget_type,
        //     window_id
        // );

        tx.send(GraphicCommand::CreateWidget {
            window_id,
            widget_id,
            parent_id,
            widget_type,
            config,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Track widget
        let widget_info = WidgetInfo {
            id: widget_id,
            window_id,
            widget_type,
            parent_id,
            children_ids: Vec::new(),
        };
        self.widgets.write().unwrap().insert(widget_id, widget_info);

        // Update parent's children list
        if let Some(pid) = parent_id {
            if let Some(parent) = self.widgets.write().unwrap().get_mut(&pid) {
                parent.children_ids.push(widget_id);
            }
        }

        // tracing::debug!("Widget {} created", widget_id);

        Ok(widget_id)
    }

    /// Update a single widget property
    ///
    /// # Arguments
    /// * `widget_id` - The widget to update
    /// * `property` - The property name
    /// * `value` - The new property value
    pub async fn update_widget_property(
        &self,
        widget_id: u64,
        property: String,
        value: PropertyValue,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "widget.setProperty() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UpdateWidgetProperty {
            widget_id,
            property,
            value,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Update multiple widget properties at once
    ///
    /// # Arguments
    /// * `widget_id` - The widget to update
    /// * `config` - New configuration (only set fields are updated)
    pub async fn update_widget_config(
        &self,
        widget_id: u64,
        config: WidgetConfig,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "widget.setConfig() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UpdateWidgetConfig {
            widget_id,
            config,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Destroy a widget and all its children
    pub async fn destroy_widget(&self, widget_id: u64) -> Result<(), String> {
        if !self.available {
            return Err(
                "widget.destroy() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::DestroyWidget {
            widget_id,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Remove widget and its children from tracking
        self.remove_widget_recursive(widget_id);

        tracing::debug!("Widget {} destroyed", widget_id);

        Ok(())
    }

    /// Helper to recursively remove widget and children from tracking
    fn remove_widget_recursive(&self, widget_id: u64) {
        let mut widgets = self.widgets.write().unwrap();

        // Get children list before removing
        let children_ids = widgets
            .get(&widget_id)
            .map(|w| w.children_ids.clone())
            .unwrap_or_default();

        // Remove from parent's children list
        if let Some(widget) = widgets.get(&widget_id) {
            if let Some(parent_id) = widget.parent_id {
                if let Some(parent) = widgets.get_mut(&parent_id) {
                    parent.children_ids.retain(|&id| id != widget_id);
                }
            }
        }

        // Remove from registry
        widgets.remove(&widget_id);

        // Remove children recursively (release lock first)
        drop(widgets);
        for child_id in children_ids {
            self.remove_widget_recursive(child_id);
        }

        // Clean up subscriptions
        self.widget_subscriptions
            .write()
            .unwrap()
            .remove_widget(widget_id);
    }

    /// Move a widget to a new parent
    pub async fn reparent_widget(
        &self,
        widget_id: u64,
        new_parent_id: Option<u64>,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "widget.reparent() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::ReparentWidget {
            widget_id,
            new_parent_id,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update tracking
        let mut widgets = self.widgets.write().unwrap();

        // Remove from old parent
        if let Some(widget) = widgets.get(&widget_id) {
            if let Some(old_parent_id) = widget.parent_id {
                if let Some(old_parent) = widgets.get_mut(&old_parent_id) {
                    old_parent.children_ids.retain(|&id| id != widget_id);
                }
            }
        }

        // Add to new parent
        if let Some(new_pid) = new_parent_id {
            if let Some(new_parent) = widgets.get_mut(&new_pid) {
                new_parent.children_ids.push(widget_id);
            }
        }

        // Update widget's parent_id
        if let Some(widget) = widgets.get_mut(&widget_id) {
            widget.parent_id = new_parent_id;
        }

        Ok(())
    }

    /// Destroy all widgets in a window
    pub async fn clear_window_widgets(&self, window_id: u64) -> Result<(), String> {
        if !self.available {
            return Err(
                "window.clearWidgets() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::ClearWindowWidgets {
            window_id,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Remove all widgets for this window from tracking
        let widget_ids: Vec<u64> = self
            .widgets
            .read()
            .unwrap()
            .iter()
            .filter(|(_, w)| w.window_id == window_id)
            .map(|(id, _)| *id)
            .collect();

        let mut widgets = self.widgets.write().unwrap();
        for id in widget_ids {
            widgets.remove(&id);
            self.widget_subscriptions.write().unwrap().remove_widget(id);
        }

        tracing::debug!("All widgets cleared from window {}", window_id);

        Ok(())
    }

    // ========================================================================
    // Widget Query
    // ========================================================================

    /// Get information about a widget
    pub fn get_widget_info(&self, widget_id: u64) -> Option<WidgetInfo> {
        self.widgets.read().unwrap().get(&widget_id).cloned()
    }

    /// Get all widgets in a window
    pub fn get_window_widgets(&self, window_id: u64) -> Vec<WidgetInfo> {
        self.widgets
            .read()
            .unwrap()
            .values()
            .filter(|w| w.window_id == window_id)
            .cloned()
            .collect()
    }

    /// Get root widgets of a window (widgets with no parent)
    pub fn get_window_root_widgets(&self, window_id: u64) -> Vec<u64> {
        self.widgets
            .read()
            .unwrap()
            .values()
            .filter(|w| w.window_id == window_id && w.parent_id.is_none())
            .map(|w| w.id)
            .collect()
    }

    /// Get a widget and all its descendants (recursive)
    ///
    /// Returns the widget ID and all children IDs recursively.
    /// Useful for cleanup operations that need to process all nested widgets.
    pub fn get_widget_and_descendants(&self, widget_id: u64) -> Vec<u64> {
        let mut result = vec![widget_id];
        let widgets = self.widgets.read().unwrap();

        if let Some(widget) = widgets.get(&widget_id) {
            for &child_id in &widget.children_ids {
                drop(widgets); // Release lock before recursive call
                result.extend(self.get_widget_and_descendants(child_id));
                return result; // Return early since we dropped the lock
            }
        }

        result
    }

    /// Helper to collect widget and descendants without lock issues
    fn collect_descendants(&self, widget_id: u64, result: &mut Vec<u64>) {
        let widgets = self.widgets.read().unwrap();
        if let Some(widget) = widgets.get(&widget_id) {
            let children = widget.children_ids.clone();
            drop(widgets);
            for child_id in children {
                result.push(child_id);
                self.collect_descendants(child_id, result);
            }
        }
    }

    /// Get all descendant IDs of a widget (children, grandchildren, etc.)
    pub fn get_widget_descendants(&self, widget_id: u64) -> Vec<u64> {
        let mut result = Vec::new();
        self.collect_descendants(widget_id, &mut result);
        result
    }

    // ========================================================================
    // Widget Event Subscription
    // ========================================================================

    /// Subscribe to widget events
    pub async fn subscribe_widget_events(
        &self,
        widget_id: u64,
        event_types: Vec<WidgetEventType>,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "widget.on*() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SubscribeWidgetEvents {
            widget_id,
            event_types: event_types.clone(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update local subscriptions
        let mut subs = self.widget_subscriptions.write().unwrap();
        for event_type in event_types {
            subs.subscribe(widget_id, event_type);
        }

        Ok(())
    }

    /// Unsubscribe from widget events
    pub async fn unsubscribe_widget_events(
        &self,
        widget_id: u64,
        event_types: Vec<WidgetEventType>,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "widget.off*() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UnsubscribeWidgetEvents {
            widget_id,
            event_types: event_types.clone(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Update local subscriptions
        let mut subs = self.widget_subscriptions.write().unwrap();
        for event_type in event_types {
            subs.unsubscribe(widget_id, event_type);
        }

        Ok(())
    }

    /// Check if a widget is subscribed to an event type
    pub fn is_subscribed(&self, widget_id: u64, event_type: WidgetEventType) -> bool {
        self.widget_subscriptions
            .read()
            .unwrap()
            .is_subscribed(widget_id, event_type)
    }

    // ========================================================================
    // Asset Management (Fonts and Images)
    // ========================================================================

    /// Load a custom font
    ///
    /// # Arguments
    /// * `path` - Font file path (relative to mod/assets folder)
    /// * `alias` - Optional alias to use for this font (default: file name without extension)
    ///
    /// # Returns
    /// * `Ok(alias)` - The alias assigned to this font
    pub async fn load_font(&self, path: String, alias: Option<String>) -> Result<String, String> {
        if !self.available {
            return Err(
                "graphic.loadFont() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call graphic.enableEngine() first.")?;

        // Get asset_root for path validation and canonicalize it
        // (asset_root may be relative like "./workspace_data/demo")
        let asset_root = self.asset_root.as_ref()
            .ok_or("Asset root not configured. This is a client configuration error.")?;
        let canonical_asset_root = asset_root.canonicalize()
            .map_err(|e| format!("Failed to canonicalize asset_root '{}': {}", asset_root.display(), e))?;

        // The path from system.getAssetsPath() is already relative to asset_root
        // (e.g., "mods/mod-id/assets/fonts/X.ttf")
        // We need to validate it's within permitted directories (security check)
        let security_config = PathSecurityConfig::new(&canonical_asset_root);

        // Build the full absolute path for validation
        let full_path = canonical_asset_root.join(&path);
        let validated_path = validate_and_resolve_path(&full_path, &security_config)?;

        // For Bevy's AssetServer, we need the path relative to asset_root
        let relative_path = validated_path
            .strip_prefix(&canonical_asset_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::LoadFont {
            path: relative_path.clone(),
            alias: alias.clone(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        let assigned_alias = response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Track loaded font
        let font_info = FontInfo {
            alias: assigned_alias.clone(),
            path: path.clone(),
            family_name: None,
        };
        self.loaded_fonts
            .write()
            .unwrap()
            .insert(assigned_alias.clone(), font_info);

        // tracing::debug!("Query Font loaded {} with alias: \"{}\"", path, assigned_alias);

        Ok(assigned_alias)
    }

    /// Unload a font
    pub async fn unload_font(&self, alias: String) -> Result<(), String> {
        if !self.available {
            return Err(
                "graphic.unloadFont() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UnloadFont {
            alias: alias.clone(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        // Remove from tracking
        self.loaded_fonts.write().unwrap().remove(&alias);

        tracing::debug!("Font unloaded: {}", alias);

        Ok(())
    }

    /// Get list of loaded fonts
    pub fn get_loaded_fonts(&self) -> Vec<FontInfo> {
        self.loaded_fonts.read().unwrap().values().cloned().collect()
    }

    /// Preload an image for faster first use
    pub async fn preload_image(&self, path: String) -> Result<(), String> {
        if !self.available {
            return Err(
                "graphic.preloadImage() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call graphic.enableEngine() first.")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::PreloadImage {
            path: path.clone(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")??;

        tracing::debug!("Image preloaded: {}", path);

        Ok(())
    }

    // ========================================================================
    // Screen/Monitor Management
    // ========================================================================

    /// Get the primary screen/monitor identifier
    ///
    /// Returns an identifier for the primary display that can be used
    /// with other screen-related methods.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Called on the server
    /// - No graphic engine is enabled
    /// - The engine fails to respond
    pub async fn get_primary_screen(&self) -> Result<u32, String> {
        if !self.available {
            return Err(
                "Graphic.getPrimaryScreen() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call Graphic.enableEngine() first.")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::GetPrimaryScreen { response_tx })
            .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Get the resolution of a specific screen/monitor
    ///
    /// # Arguments
    /// * `screen_id` - Screen identifier (from get_primary_screen or similar)
    ///
    /// # Returns
    /// Tuple of (width, height) in pixels
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Called on the server
    /// - No graphic engine is enabled
    /// - The screen ID is invalid
    /// - The engine fails to respond
    pub async fn get_screen_resolution(&self, screen_id: u32) -> Result<(u32, u32), String> {
        if !self.available {
            return Err(
                "Graphic.getScreenResolution() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call Graphic.enableEngine() first.")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::GetScreenResolution {
            screen_id,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }
}

// GraphicProxy is Send + Sync because all internal state is protected
unsafe impl Send for GraphicProxy {}
unsafe impl Sync for GraphicProxy {}
