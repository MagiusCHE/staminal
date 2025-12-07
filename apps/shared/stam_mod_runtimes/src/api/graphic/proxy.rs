//! GraphicProxy - Central coordinator for graphic operations
//!
//! The GraphicProxy provides a language-agnostic API for graphic operations,
//! routing commands to the active graphic engine via message channels.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use super::ecs::{ComponentSchema, DeclaredSystem, QueryOptions, QueryResult};
use super::{
    FontInfo, GraphicCommand, GraphicEngineInfo, GraphicEngines, GraphicEvent,
    InitialWindowConfig, WindowConfig, WindowInfo, WindowMode,
};
use super::super::path_security::{PathSecurityConfig, validate_and_resolve_path};
use super::super::resource::{ResourceInfo, ResourceType};

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

    /// Loaded fonts (alias -> FontInfo)
    loaded_fonts: Arc<RwLock<HashMap<String, FontInfo>>>,

    /// Flag: graphic proxy is available (client-only)
    available: bool,

    /// Root directory for loading assets (e.g., data_dir containing mods)
    asset_root: Option<std::path::PathBuf>,

    /// ID of the main window (used by getEngineInfo().mainWindow)
    /// Initially set to 1 (the primary window created at engine startup)
    /// Can be changed via setMainWindow() to promote a different window
    main_window_id: AtomicU64,
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
            loaded_fonts: Arc::new(RwLock::new(HashMap::new())),
            available: true,
            asset_root,
            main_window_id: AtomicU64::new(1), // Primary window created at engine startup
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
            loaded_fonts: Arc::new(RwLock::new(HashMap::new())),
            available: false,
            asset_root: None,
            main_window_id: AtomicU64::new(1),
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

    /// Get the current main window ID
    ///
    /// Returns the ID of the window that is considered the "main" window.
    /// This is used by `getEngineInfo().mainWindow` in JavaScript.
    pub fn get_main_window_id(&self) -> u64 {
        self.main_window_id.load(Ordering::SeqCst)
    }

    /// Set the main window
    ///
    /// Promotes a window to be the "main" window. This affects:
    /// - `getEngineInfo().mainWindow` will return this window
    /// - The `GraphicEngineWindowClosed` event will report this as the main window
    ///
    /// # Arguments
    /// * `window_id` - The ID of the window to promote as main window
    ///
    /// # Returns
    /// Ok(()) if the window exists, Err if not found or not available
    pub fn set_main_window(&self, window_id: u64) -> Result<(), String> {
        if !self.available {
            return Err(
                "Graphic.setMainWindow() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        // Verify the window exists
        let windows = self.windows.read().unwrap();
        if !windows.contains_key(&window_id) {
            return Err(format!("Window {} not found", window_id));
        }

        self.main_window_id.store(window_id, Ordering::SeqCst);
        tracing::debug!("Main window set to {}", window_id);
        Ok(())
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
            info.config.mode = mode;
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
    // Resource Loading (for ResourceProxy)
    // ========================================================================

    /// Load a resource into the graphic engine's cache
    ///
    /// This method is called by ResourceProxy to load graphic resources
    /// (images, fonts, shaders, etc.) into the engine's ResourceRegistry.
    ///
    /// # Arguments
    /// * `path` - The resolved file path (already validated by ResourceProxy)
    /// * `alias` - The unique alias for this resource
    /// * `resource_type` - The type of resource to load
    /// * `asset_id` - The unique asset ID (generated by ResourceProxy)
    /// * `force_reload` - If true, reload even if already cached
    ///
    /// # Returns
    /// * `Ok(ResourceInfo)` - Information about the loaded resource
    pub async fn load_resource(
        &self,
        path: String,
        alias: String,
        resource_type: ResourceType,
        asset_id: u64,
        force_reload: bool,
    ) -> Result<ResourceInfo, String> {
        if !self.available {
            return Err(
                "Resource.load() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call Graphic.enableEngine() first.")?;

        // Get asset_root for path validation and canonicalize it
        let asset_root = self.asset_root.as_ref()
            .ok_or("Asset root not configured. This is a client configuration error.")?;
        let canonical_asset_root = asset_root.canonicalize()
            .map_err(|e| format!("Failed to canonicalize asset_root '{}': {}", asset_root.display(), e))?;

        // Validate the path is within permitted directories (security check)
        let security_config = PathSecurityConfig::new(&canonical_asset_root);
        let full_path = canonical_asset_root.join(&path);
        let validated_path = validate_and_resolve_path(&full_path, &security_config)?;

        // For Bevy's AssetServer, we need the path relative to asset_root
        let relative_path = validated_path
            .strip_prefix(&canonical_asset_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::LoadResource {
            path: relative_path,
            alias,
            resource_type,
            asset_id,
            force_reload,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Unload a resource from the graphic engine's cache
    ///
    /// This method is called by ResourceProxy to release graphic resources.
    /// The engine will drop the handle, allowing Bevy to garbage collect
    /// the asset if no other handles remain.
    ///
    /// # Arguments
    /// * `asset_id` - The asset ID to unload
    pub async fn unload_resource(&self, asset_id: u64) -> Result<(), String> {
        if !self.available {
            return Err(
                "Resource.unload() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UnloadResource {
            asset_id,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Unload all resources from the graphic engine's cache
    ///
    /// This method is called by ResourceProxy when clearing all resources.
    pub async fn unload_all_resources(&self) -> Result<(), String> {
        if !self.available {
            return Err(
                "Resource.unloadAll() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UnloadAllResources { response_tx })
            .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Get the asset root path
    ///
    /// Returns the root directory for loading assets. This is used by
    /// ResourceProxy to resolve relative paths.
    pub fn get_asset_root(&self) -> Option<&std::path::PathBuf> {
        self.asset_root.as_ref()
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

    // ========================================================================
    // ECS Operations
    // ========================================================================

    /// Spawn a new entity
    ///
    /// Creates a new entity with optional initial components.
    /// Returns the entity's script-facing ID.
    ///
    /// # Arguments
    /// * `components` - Initial components to add (component_name -> JSON data)
    /// * `owner_mod` - The mod that owns this entity
    /// * `parent` - Optional parent entity ID. If None and this is a UI entity,
    ///              it will be parented to the main window's root.
    ///
    /// # Returns
    /// * `Ok(entity_id)` - The ID of the spawned entity
    pub async fn spawn_entity(
        &self,
        components: HashMap<String, serde_json::Value>,
        owner_mod: String,
        parent: Option<u64>,
    ) -> Result<u64, String> {
        if !self.available {
            return Err(
                "World.spawn() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx
            .as_ref()
            .ok_or("No graphic engine enabled. Call Graphic.enableEngine() first.")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SpawnEntity {
            components,
            owner_mod,
            parent,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Despawn an entity
    ///
    /// Removes an entity and all its components from the world.
    ///
    /// # Arguments
    /// * `entity_id` - The entity to despawn
    pub async fn despawn_entity(&self, entity_id: u64) -> Result<(), String> {
        if !self.available {
            return Err(
                "World.despawn() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::DespawnEntity {
            entity_id,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Insert a component on an entity
    ///
    /// Adds or replaces a component on an existing entity.
    ///
    /// # Arguments
    /// * `entity_id` - The entity to modify
    /// * `component_name` - The component type name
    /// * `component_data` - The component data as JSON
    pub async fn insert_component(
        &self,
        entity_id: u64,
        component_name: String,
        component_data: serde_json::Value,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "entity.insert() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::InsertComponent {
            entity_id,
            component_name,
            component_data,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Update specific fields of a component on an entity (merge with existing)
    ///
    /// Unlike `insert_component` which replaces the entire component,
    /// this method merges the provided fields with existing component data.
    ///
    /// # Arguments
    /// * `entity_id` - The entity to modify
    /// * `component_name` - The component type name
    /// * `component_data` - Partial component data to merge
    pub async fn update_component(
        &self,
        entity_id: u64,
        component_name: String,
        component_data: serde_json::Value,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "entity.update() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UpdateComponent {
            entity_id,
            component_name,
            component_data,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Remove a component from an entity
    ///
    /// # Arguments
    /// * `entity_id` - The entity to modify
    /// * `component_name` - The component type name to remove
    pub async fn remove_component(
        &self,
        entity_id: u64,
        component_name: String,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "entity.remove() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::RemoveComponent {
            entity_id,
            component_name,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Get a component's data from an entity
    ///
    /// # Arguments
    /// * `entity_id` - The entity to query
    /// * `component_name` - The component type name
    ///
    /// # Returns
    /// * `Ok(Some(data))` - The component data
    /// * `Ok(None)` - The entity doesn't have this component
    pub async fn get_component(
        &self,
        entity_id: u64,
        component_name: String,
    ) -> Result<Option<serde_json::Value>, String> {
        if !self.available {
            return Err(
                "entity.get() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::GetComponent {
            entity_id,
            component_name,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Check if an entity has a component
    ///
    /// # Arguments
    /// * `entity_id` - The entity to query
    /// * `component_name` - The component type name
    pub async fn has_component(
        &self,
        entity_id: u64,
        component_name: String,
    ) -> Result<bool, String> {
        if !self.available {
            return Err(
                "entity.has() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::HasComponent {
            entity_id,
            component_name,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Query entities matching criteria
    ///
    /// Returns all entities that have the required components
    /// and don't have the excluded components.
    ///
    /// # Arguments
    /// * `options` - Query options (with/without components, limit)
    pub async fn query_entities(&self, options: QueryOptions) -> Result<Vec<QueryResult>, String> {
        if !self.available {
            return Err(
                "World.query() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::QueryEntities {
            options,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Register a custom component type
    ///
    /// Components must be registered with a schema before they can be used.
    ///
    /// # Arguments
    /// * `schema` - The component schema (name and field definitions)
    pub async fn register_component(&self, schema: ComponentSchema) -> Result<(), String> {
        if !self.available {
            return Err(
                "World.registerComponent() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::RegisterComponent { schema, response_tx })
            .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Declare a system to be executed by the engine
    ///
    /// Systems can use predefined behaviors or mathematical formulas.
    ///
    /// # Arguments
    /// * `system` - The system configuration
    pub async fn declare_system(&self, system: DeclaredSystem) -> Result<(), String> {
        if !self.available {
            return Err(
                "World.declareSystem() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::DeclareSystem { system, response_tx })
            .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Enable or disable a declared system
    ///
    /// # Arguments
    /// * `name` - The system name
    /// * `enabled` - Whether the system should be enabled
    pub async fn set_system_enabled(&self, name: String, enabled: bool) -> Result<(), String> {
        if !self.available {
            return Err(
                "World.setSystemEnabled() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::SetSystemEnabled {
            name,
            enabled,
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Remove a declared system
    ///
    /// # Arguments
    /// * `name` - The system name to remove
    pub async fn remove_system(&self, name: String) -> Result<(), String> {
        if !self.available {
            return Err(
                "World.removeSystem() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::RemoveSystem { name, response_tx })
            .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    // ========================================================================
    // Entity Event Callback Registration
    // ========================================================================

    /// Register an event callback for an entity
    ///
    /// When registered, the engine will send EntityEventCallback events for this
    /// entity instead of generic EntityInteractionChanged events.
    /// This enables direct callback dispatch without global event broadcasting.
    ///
    /// # Arguments
    /// * `entity_id` - The entity to register a callback for
    /// * `event_type` - The event type (e.g., "click", "hover", "enter", "leave")
    pub async fn register_entity_event_callback(&self, entity_id: u64, event_type: &str) -> Result<(), String> {
        if !self.available {
            return Err(
                "World.registerEntityEventCallback() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::RegisterEntityEventCallback {
            entity_id,
            event_type: event_type.to_string(),
            response_tx,
        })
        .map_err(|_| "Failed to send command to graphic engine")?;

        response_rx
            .await
            .map_err(|_| "Graphic engine did not respond")?
    }

    /// Unregister an event callback for an entity
    ///
    /// After unregistering, the engine will revert to sending generic
    /// EntityInteractionChanged events for this entity.
    ///
    /// # Arguments
    /// * `entity_id` - The entity to unregister
    /// * `event_type` - The event type to unregister
    pub async fn unregister_entity_event_callback(&self, entity_id: u64, event_type: &str) -> Result<(), String> {
        if !self.available {
            return Err(
                "World.unregisterEntityEventCallback() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        let tx = self.command_tx.read().unwrap();
        let tx = tx.as_ref().ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        tx.send(GraphicCommand::UnregisterEntityEventCallback {
            entity_id,
            event_type: event_type.to_string(),
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
