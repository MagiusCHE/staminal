//! GraphicProxy - Central coordinator for graphic operations
//!
//! The GraphicProxy is shared across all mod contexts and ALL scripting runtimes
//! (JavaScript, Lua, C#, etc.). It provides a unified interface for graphic
//! operations regardless of the underlying engine or calling language.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, Mutex};

use super::command::GraphicCommand;
use super::engines::{EngineFactory, GraphicEngine, GraphicEngines};
use super::event::GraphicEvent;
use super::input::FrameSnapshot;
use super::window::{WindowConfig, WindowInfo, WindowPositionMode};

/// Channel buffer size for commands and events
const CHANNEL_BUFFER_SIZE: usize = 256;

/// Central proxy for graphic engine operations
///
/// This struct is shared across all mod contexts and ALL scripting runtimes.
/// It provides a unified interface for graphic operations regardless of the
/// underlying engine or calling language.
#[derive(Clone)]
pub struct GraphicProxy {
    inner: Arc<GraphicProxyInner>,
}

/// Engine pending main thread execution
///
/// When an engine requires the main thread, it is stored here along with
/// its channels. The main application loop is responsible for calling
/// `take_pending_main_thread_engine()` and running it.
pub struct PendingMainThreadEngine {
    /// The engine to run
    pub engine: Box<dyn GraphicEngine>,
    /// Receiver for commands from the proxy
    pub command_rx: mpsc::Receiver<GraphicCommand>,
    /// Sender for events back to the proxy
    pub event_tx: mpsc::Sender<GraphicEvent>,
}

struct GraphicProxyInner {
    /// Currently active engine type (None if no engine enabled)
    active_engine: RwLock<Option<GraphicEngines>>,

    /// Channel to send commands to the graphic engine thread
    command_tx: RwLock<Option<mpsc::Sender<GraphicCommand>>>,

    /// Channel to receive events from the graphic engine thread
    event_rx: Mutex<Option<mpsc::Receiver<GraphicEvent>>>,

    /// Handle to the engine thread (for cleanup)
    engine_thread: RwLock<Option<JoinHandle<()>>>,

    /// Window registry - maps window IDs to their state
    windows: RwLock<HashMap<u64, WindowInfo>>,

    /// Next window ID counter
    next_window_id: AtomicU64,

    /// Current frame snapshot (for optimized access during frame callbacks)
    frame_snapshot: RwLock<FrameSnapshot>,

    /// Whether engine is ready
    engine_ready: RwLock<bool>,

    /// Registered engine factories
    /// Maps engine type to factory function that creates the engine instance
    engine_factories: RwLock<HashMap<GraphicEngines, EngineFactory>>,

    /// Whether an engine requiring main thread is pending
    /// The main loop should check this and call take_pending_main_thread_engine()
    has_pending_main_thread_engine: AtomicBool,

    /// Engine pending main thread execution (if any)
    pending_main_thread_engine: Mutex<Option<PendingMainThreadEngine>>,
}

impl GraphicProxy {
    /// Create a new GraphicProxy
    pub fn new() -> Self {
        Self {
            inner: Arc::new(GraphicProxyInner {
                active_engine: RwLock::new(None),
                command_tx: RwLock::new(None),
                event_rx: Mutex::new(None),
                engine_thread: RwLock::new(None),
                windows: RwLock::new(HashMap::new()),
                next_window_id: AtomicU64::new(1),
                frame_snapshot: RwLock::new(FrameSnapshot::new()),
                engine_ready: RwLock::new(false),
                engine_factories: RwLock::new(HashMap::new()),
                has_pending_main_thread_engine: AtomicBool::new(false),
                pending_main_thread_engine: Mutex::new(None),
            }),
        }
    }

    /// Register an engine factory
    ///
    /// This allows the client to register engine implementations that can be
    /// instantiated when a script calls `system.enable_graphic_engine(type)`.
    ///
    /// # Arguments
    /// * `engine_type` - The engine type to register
    /// * `factory` - A factory function that creates the engine instance
    pub fn register_engine_factory<F>(&self, engine_type: GraphicEngines, factory: F)
    where
        F: Fn() -> Box<dyn GraphicEngine> + Send + Sync + 'static,
    {
        let mut factories = self.inner.engine_factories.write().unwrap();
        factories.insert(engine_type, Arc::new(factory));
        tracing::debug!("Registered engine factory for {:?}", engine_type);
    }

    /// Enable a graphic engine by type
    ///
    /// This looks up the registered factory for the given engine type,
    /// creates an instance, and starts it in a separate thread.
    ///
    /// # Arguments
    /// * `engine_type` - The engine type to enable
    ///
    /// # Returns
    /// * `Ok(())` if engine was enabled successfully
    /// * `Err(String)` if no factory is registered, engine is already enabled, or spawn failed
    pub async fn enable_by_type(&self, engine_type: GraphicEngines) -> Result<(), String> {
        // Get the factory for this engine type
        let factory = {
            let factories = self.inner.engine_factories.read().unwrap();
            factories.get(&engine_type).cloned()
        };

        let factory = factory.ok_or_else(|| {
            format!(
                "No engine factory registered for {:?}. Available engines: {:?}",
                engine_type,
                self.inner
                    .engine_factories
                    .read()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>()
            )
        })?;

        // Create the engine instance using the factory
        let engine = factory();

        // Enable it using the existing enable method
        self.enable_boxed(engine).await
    }

    /// Enable a graphic engine from a boxed trait object
    ///
    /// This is used internally by `enable_by_type` and can also be called directly
    /// if you have a boxed engine instance.
    ///
    /// If the engine requires main thread execution (via `require_main_thread()`),
    /// the engine is stored as pending and the caller must retrieve it using
    /// `take_pending_main_thread_engine()` and run it on the main thread.
    pub async fn enable_boxed(&self, engine: Box<dyn GraphicEngine>) -> Result<(), String> {
        // Check if already enabled
        {
            let active = self.inner.active_engine.read().unwrap();
            if active.is_some() {
                return Err("Graphic engine already enabled".to_string());
            }
        }

        // Create channels
        let (command_tx, command_rx) = mpsc::channel::<GraphicCommand>(CHANNEL_BUFFER_SIZE);
        let (event_tx, event_rx) = mpsc::channel::<GraphicEvent>(CHANNEL_BUFFER_SIZE);

        let engine_type = engine.engine_type();
        let requires_main_thread = engine.require_main_thread();

        // Store state common to both paths
        {
            *self.inner.active_engine.write().unwrap() = Some(engine_type);
            *self.inner.command_tx.write().unwrap() = Some(command_tx);
            *self.inner.event_rx.lock().await = Some(event_rx);
        }

        if requires_main_thread {
            // Store engine for main thread execution
            // The main loop will call take_pending_main_thread_engine() to get it
            tracing::debug!(
                "Engine {:?} requires main thread - storing as pending",
                engine_type
            );
            {
                let mut pending = self.inner.pending_main_thread_engine.lock().await;
                *pending = Some(PendingMainThreadEngine {
                    engine,
                    command_rx,
                    event_tx,
                });
            }
            self.inner
                .has_pending_main_thread_engine
                .store(true, Ordering::Release);
        } else {
            // Spawn engine in a separate thread (existing behavior)
            let mut engine = engine;
            let thread_handle = std::thread::Builder::new()
                .name(format!("graphic-{}", engine_type.name().to_lowercase()))
                .spawn(move || {
                    engine.run(command_rx, event_tx);
                })
                .map_err(|e| format!("Failed to spawn engine thread: {}", e))?;

            *self.inner.engine_thread.write().unwrap() = Some(thread_handle);
        }

        Ok(())
    }

    /// Check if a graphic engine is currently enabled
    pub fn is_enabled(&self) -> bool {
        self.inner.active_engine.read().unwrap().is_some()
    }

    /// Get the currently active engine type
    pub fn active_engine(&self) -> Option<GraphicEngines> {
        *self.inner.active_engine.read().unwrap()
    }

    /// Check if engine is ready to receive commands
    pub fn is_ready(&self) -> bool {
        *self.inner.engine_ready.read().unwrap()
    }

    /// Check if there is an engine pending main thread execution
    ///
    /// When this returns true, the main loop should call
    /// `take_pending_main_thread_engine()` and run the engine.
    pub fn has_pending_main_thread_engine(&self) -> bool {
        self.inner
            .has_pending_main_thread_engine
            .load(Ordering::Acquire)
    }

    /// Take the pending main thread engine (if any)
    ///
    /// This should be called from the main thread when
    /// `has_pending_main_thread_engine()` returns true.
    ///
    /// Returns the engine, command receiver, and event sender.
    /// The caller must call `engine.run(command_rx, event_tx)` on the main thread.
    pub async fn take_pending_main_thread_engine(&self) -> Option<PendingMainThreadEngine> {
        let pending = {
            let mut guard = self.inner.pending_main_thread_engine.lock().await;
            guard.take()
        };

        if pending.is_some() {
            self.inner
                .has_pending_main_thread_engine
                .store(false, Ordering::Release);
        }

        pending
    }

    /// Enable a graphic engine
    ///
    /// This spawns the engine in a separate thread and sets up communication channels.
    ///
    /// # Arguments
    /// * `engine` - The engine implementation to use
    ///
    /// # Returns
    /// * `Ok(())` if engine was enabled successfully
    /// * `Err(String)` if an engine is already enabled or spawn failed
    pub fn enable<E: GraphicEngine>(&self, mut engine: E) -> Result<(), String> {
        // Check if already enabled
        {
            let active = self.inner.active_engine.read().unwrap();
            if active.is_some() {
                return Err("Graphic engine already enabled".to_string());
            }
        }

        // Create channels
        let (command_tx, command_rx) = mpsc::channel::<GraphicCommand>(CHANNEL_BUFFER_SIZE);
        let (event_tx, event_rx) = mpsc::channel::<GraphicEvent>(CHANNEL_BUFFER_SIZE);

        let engine_type = engine.engine_type();

        // Spawn engine thread
        let thread_handle = std::thread::Builder::new()
            .name(format!("graphic-{}", engine_type.name().to_lowercase()))
            .spawn(move || {
                engine.run(command_rx, event_tx);
            })
            .map_err(|e| format!("Failed to spawn engine thread: {}", e))?;

        // Store state
        {
            *self.inner.active_engine.write().unwrap() = Some(engine_type);
            *self.inner.command_tx.write().unwrap() = Some(command_tx);
            *self.inner.event_rx.blocking_lock() = Some(event_rx);
            *self.inner.engine_thread.write().unwrap() = Some(thread_handle);
        }

        Ok(())
    }

    /// Poll for events from the graphic engine
    ///
    /// This should be called from the main event loop to process
    /// events from the engine (window events, input, frame updates).
    ///
    /// # Returns
    /// Vector of events received since last poll
    pub async fn poll_events(&self) -> Vec<GraphicEvent> {
        let mut events = Vec::new();
        let mut event_rx = self.inner.event_rx.lock().await;

        if let Some(rx) = event_rx.as_mut() {
            while let Ok(event) = rx.try_recv() {
                // Handle internal state updates
                match &event {
                    GraphicEvent::EngineReady => {
                        *self.inner.engine_ready.write().unwrap() = true;
                    }
                    GraphicEvent::EngineShuttingDown => {
                        *self.inner.engine_ready.write().unwrap() = false;
                    }
                    GraphicEvent::FrameStart { snapshot } => {
                        *self.inner.frame_snapshot.write().unwrap() = snapshot.clone();
                    }
                    GraphicEvent::WindowCreated { window_id } => {
                        if let Some(window) = self.inner.windows.write().unwrap().get_mut(window_id)
                        {
                            window.created = true;
                        }
                    }
                    GraphicEvent::WindowClosed { window_id } => {
                        self.inner.windows.write().unwrap().remove(window_id);
                    }
                    GraphicEvent::WindowResized {
                        window_id,
                        width,
                        height,
                    } => {
                        if let Some(window) = self.inner.windows.write().unwrap().get_mut(window_id)
                        {
                            window.config.width = *width;
                            window.config.height = *height;
                        }
                    }
                    GraphicEvent::WindowMoved { window_id, x, y } => {
                        if let Some(window) = self.inner.windows.write().unwrap().get_mut(window_id)
                        {
                            window.x = *x;
                            window.y = *y;
                        }
                    }
                    GraphicEvent::WindowFocused { window_id, focused } => {
                        if let Some(window) = self.inner.windows.write().unwrap().get_mut(window_id)
                        {
                            window.focused = *focused;
                        }
                    }
                    _ => {}
                }

                events.push(event);
            }
        }

        events
    }

    /// Wait for and receive the next event from the graphic engine
    ///
    /// This is an async method that blocks until an event is available.
    /// Use this in a `tokio::select!` to handle engine events alongside
    /// other async tasks.
    ///
    /// Note: If no engine is enabled or the engine is pending on the main thread,
    /// this will wait indefinitely (use with `tokio::select!`).
    ///
    /// # Returns
    /// * `Some(event)` - The next event from the engine
    /// * `None` - The event channel was closed (engine shutdown)
    pub async fn recv_event(&self) -> Option<GraphicEvent> {
        loop {
            // If engine is pending on main thread, we can't receive events yet
            // because the engine hasn't started. Yield and let the caller's
            // select! loop run the check for pending engine.
            if self.has_pending_main_thread_engine() {
                // Yield to allow other branches in select! to run
                // The caller should check has_pending_main_thread_engine() and
                // call take_pending_main_thread_engine()
                tokio::task::yield_now().await;
                // Small delay to avoid busy-loop
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }

            // Try to acquire the lock - don't hold it while pending
            let mut event_rx_guard = self.inner.event_rx.lock().await;

            // If no receiver is set, no engine is active - wait forever
            let rx = match event_rx_guard.as_mut() {
                Some(rx) => rx,
                None => {
                    // Release the lock before waiting forever
                    drop(event_rx_guard);
                    std::future::pending::<()>().await;
                    return None;
                }
            };

            // Use timeout to periodically check if engine became pending again
            // This handles the edge case where engine is disabled and re-enabled
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Some(event)) => {
                    // Handle internal state updates
                    match &event {
                        GraphicEvent::EngineReady => {
                            *self.inner.engine_ready.write().unwrap() = true;
                        }
                        GraphicEvent::EngineShuttingDown => {
                            *self.inner.engine_ready.write().unwrap() = false;
                        }
                        GraphicEvent::FrameStart { snapshot } => {
                            *self.inner.frame_snapshot.write().unwrap() = snapshot.clone();
                        }
                        GraphicEvent::WindowCreated { window_id } => {
                            if let Some(window) =
                                self.inner.windows.write().unwrap().get_mut(window_id)
                            {
                                window.created = true;
                            }
                        }
                        GraphicEvent::WindowClosed { window_id } => {
                            self.inner.windows.write().unwrap().remove(window_id);
                        }
                        GraphicEvent::WindowResized {
                            window_id,
                            width,
                            height,
                        } => {
                            if let Some(window) =
                                self.inner.windows.write().unwrap().get_mut(window_id)
                            {
                                window.config.width = *width;
                                window.config.height = *height;
                            }
                        }
                        GraphicEvent::WindowFocused { window_id, focused } => {
                            if let Some(window) =
                                self.inner.windows.write().unwrap().get_mut(window_id)
                            {
                                window.focused = *focused;
                            }
                        }
                        _ => {}
                    }
                    return Some(event);
                }
                Ok(None) => {
                    // Channel closed - engine died
                    return None;
                }
                Err(_) => {
                    // Timeout - release lock and loop to check pending status
                    continue;
                }
            }
        }
    }

    /// Get the current frame snapshot
    ///
    /// This is optimized for use during frame callbacks where
    /// input state needs to be accessed multiple times.
    pub fn frame_snapshot(&self) -> FrameSnapshot {
        self.inner.frame_snapshot.read().unwrap().clone()
    }

    /// Create a new window
    ///
    /// # Arguments
    /// * `config` - Window configuration
    ///
    /// # Returns
    /// * `Ok(window_id)` - The unique ID of the created window
    /// * `Err(String)` - If no engine is enabled or creation failed
    pub async fn create_window(&self, config: WindowConfig) -> Result<u64, String> {
        tracing::debug!("GraphicProxy::create_window called with config: {:?}", config);

        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let window_id = self.inner.next_window_id.fetch_add(1, Ordering::SeqCst);
        tracing::debug!("Assigned window_id: {}", window_id);

        // Register window locally
        {
            let mut windows = self.inner.windows.write().unwrap();
            windows.insert(window_id, WindowInfo::new(window_id, config.clone()));
        }

        let (response_tx, response_rx) = oneshot::channel();

        tracing::debug!("Sending CreateWindow command to engine...");
        command_tx
            .send(GraphicCommand::CreateWindow {
                id: window_id,
                config,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        // If engine is pending for main thread, don't wait for response
        // The command is queued and will be processed when the engine starts
        if self.has_pending_main_thread_engine() {
            tracing::debug!("Engine pending on main thread - returning immediately with window_id: {}", window_id);
            return Ok(window_id);
        }

        tracing::debug!("Waiting for engine response...");
        let result = response_rx
            .await
            .map_err(|_| "Engine did not respond")?
            .map(|_| window_id);

        tracing::debug!("CreateWindow result: {:?}", result);
        result
    }

    /// Close a window
    pub async fn close_window(&self, window_id: u64) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::CloseWindow {
                id: window_id,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Set window size
    pub async fn set_window_size(
        &self,
        window_id: u64,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::SetWindowSize {
                id: window_id,
                width,
                height,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Set window title
    pub async fn set_window_title(&self, window_id: u64, title: String) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::SetWindowTitle {
                id: window_id,
                title,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Set window fullscreen mode
    pub async fn set_window_fullscreen(
        &self,
        window_id: u64,
        fullscreen: bool,
    ) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::SetWindowFullscreen {
                id: window_id,
                fullscreen,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Set window visibility
    pub async fn set_window_visible(&self, window_id: u64, visible: bool) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::SetWindowVisible {
                id: window_id,
                visible,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Set window position
    pub async fn set_window_position(&self, window_id: u64, x: i32, y: i32) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::SetWindowPosition {
                id: window_id,
                x,
                y,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Set window position mode (centered, etc.)
    pub async fn set_window_position_mode(
        &self,
        window_id: u64,
        mode: WindowPositionMode,
    ) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::SetWindowPositionMode {
                id: window_id,
                mode,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Set window resizable property
    pub async fn set_window_resizable(
        &self,
        window_id: u64,
        resizable: bool,
    ) -> Result<(), String> {
        let command_tx = self
            .inner
            .command_tx
            .read()
            .unwrap()
            .clone()
            .ok_or("No graphic engine enabled")?;

        let (response_tx, response_rx) = oneshot::channel();

        command_tx
            .send(GraphicCommand::SetWindowResizable {
                id: window_id,
                resizable,
                response_tx,
            })
            .await
            .map_err(|_| "Failed to send command to engine")?;

        response_rx.await.map_err(|_| "Engine did not respond")?
    }

    /// Get window info
    pub fn get_window(&self, window_id: u64) -> Option<WindowInfo> {
        self.inner.windows.read().unwrap().get(&window_id).cloned()
    }

    /// Get all window IDs
    pub fn window_ids(&self) -> Vec<u64> {
        self.inner.windows.read().unwrap().keys().copied().collect()
    }

    /// Shutdown the graphic engine gracefully
    ///
    /// # Arguments
    /// * `timeout` - Maximum time to wait for graceful shutdown
    ///
    /// # Returns
    /// * `Ok(())` if shutdown completed
    /// * `Err(String)` if timeout or failure
    pub async fn shutdown(&self, timeout: Duration) -> Result<(), String> {
        // Check if engine is running
        let command_tx = {
            let tx = self.inner.command_tx.read().unwrap();
            tx.clone()
        };

        if let Some(tx) = command_tx {
            let (response_tx, response_rx) = oneshot::channel();

            // Send shutdown command
            if tx
                .send(GraphicCommand::Shutdown { response_tx })
                .await
                .is_err()
            {
                // Channel closed, engine already dead
                return Ok(());
            }

            // Wait for response with timeout
            match tokio::time::timeout(timeout, response_rx).await {
                Ok(Ok(result)) => result?,
                Ok(Err(_)) => {
                    // Channel closed, engine exited
                }
                Err(_) => {
                    // Timeout - engine did not respond
                    tracing::warn!("Graphic engine shutdown timed out");
                }
            }
        }

        // Clean up state
        *self.inner.active_engine.write().unwrap() = None;
        *self.inner.command_tx.write().unwrap() = None;
        *self.inner.event_rx.lock().await = None;
        *self.inner.engine_ready.write().unwrap() = false;

        // Wait for thread to finish (with timeout)
        if let Some(handle) = self.inner.engine_thread.write().unwrap().take() {
            // We can't async wait on a JoinHandle, so we use a blocking approach
            // This should be fine since the engine should be shutting down
            let _ = handle.join();
        }

        Ok(())
    }
}

impl Default for GraphicProxy {
    fn default() -> Self {
        Self::new()
    }
}
