//! GraphicApi - High-level API wrapper for GraphicProxy
//!
//! This module provides a cloneable API wrapper that can be passed to
//! scripting runtimes while keeping the GraphicProxy as a single instance.

use std::sync::Arc;
use super::engines::GraphicEngines;
use super::proxy::GraphicProxy;
use super::window::WindowConfig;
use super::input::FrameSnapshot;

/// High-level API for graphic operations
///
/// This is a lightweight wrapper around `GraphicProxy` that can be cloned
/// and passed to scripting runtimes. The underlying `GraphicProxy` is shared
/// via `Arc`.
#[derive(Clone)]
pub struct GraphicApi {
    proxy: Arc<GraphicProxy>,
}

impl GraphicApi {
    /// Create a new GraphicApi wrapping a GraphicProxy
    pub fn new(proxy: GraphicProxy) -> Self {
        Self {
            proxy: Arc::new(proxy),
        }
    }

    /// Create a new GraphicApi from an existing Arc<GraphicProxy>
    pub fn from_arc(proxy: Arc<GraphicProxy>) -> Self {
        Self { proxy }
    }

    /// Get a reference to the underlying proxy
    pub fn proxy(&self) -> &GraphicProxy {
        &self.proxy
    }

    /// Get the Arc to the underlying proxy
    pub fn proxy_arc(&self) -> Arc<GraphicProxy> {
        Arc::clone(&self.proxy)
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
        self.proxy.enable_by_type(engine_type).await
    }

    /// Check if a graphic engine is currently enabled
    pub fn is_enabled(&self) -> bool {
        self.proxy.is_enabled()
    }

    /// Check if engine is ready to receive commands
    pub fn is_ready(&self) -> bool {
        self.proxy.is_ready()
    }

    /// Get the currently active engine type
    pub fn active_engine(&self) -> Option<super::engines::GraphicEngines> {
        self.proxy.active_engine()
    }

    /// Get the current frame snapshot
    pub fn frame_snapshot(&self) -> FrameSnapshot {
        self.proxy.frame_snapshot()
    }

    /// Create a new window
    pub async fn create_window(&self, config: WindowConfig) -> Result<u64, String> {
        self.proxy.create_window(config).await
    }

    /// Close a window
    pub async fn close_window(&self, window_id: u64) -> Result<(), String> {
        self.proxy.close_window(window_id).await
    }

    /// Set window size
    pub async fn set_window_size(
        &self,
        window_id: u64,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.proxy.set_window_size(window_id, width, height).await
    }

    /// Set window title
    pub async fn set_window_title(&self, window_id: u64, title: String) -> Result<(), String> {
        self.proxy.set_window_title(window_id, title).await
    }

    /// Set window fullscreen mode
    pub async fn set_window_fullscreen(
        &self,
        window_id: u64,
        fullscreen: bool,
    ) -> Result<(), String> {
        self.proxy.set_window_fullscreen(window_id, fullscreen).await
    }

    /// Set window visibility
    pub async fn set_window_visible(&self, window_id: u64, visible: bool) -> Result<(), String> {
        self.proxy.set_window_visible(window_id, visible).await
    }

    /// Set window position
    pub async fn set_window_position(&self, window_id: u64, x: i32, y: i32) -> Result<(), String> {
        self.proxy.set_window_position(window_id, x, y).await
    }

    /// Set window position mode (centered, etc.)
    pub async fn set_window_position_mode(
        &self,
        window_id: u64,
        mode: super::window::WindowPositionMode,
    ) -> Result<(), String> {
        self.proxy.set_window_position_mode(window_id, mode).await
    }

    /// Set window resizable property
    pub async fn set_window_resizable(&self, window_id: u64, resizable: bool) -> Result<(), String> {
        self.proxy.set_window_resizable(window_id, resizable).await
    }

    /// Get window info
    pub fn get_window(&self, window_id: u64) -> Option<super::window::WindowInfo> {
        self.proxy.get_window(window_id)
    }

    /// Get all window IDs
    pub fn window_ids(&self) -> Vec<u64> {
        self.proxy.window_ids()
    }

    /// Shutdown the graphic engine gracefully
    pub async fn shutdown(&self, timeout: std::time::Duration) -> Result<(), String> {
        self.proxy.shutdown(timeout).await
    }
}
