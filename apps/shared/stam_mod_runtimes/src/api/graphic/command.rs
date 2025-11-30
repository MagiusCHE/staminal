//! Graphic Commands
//!
//! Commands sent from the main thread to the graphic engine thread.

use tokio::sync::oneshot;
use super::window::{WindowConfig, WindowPositionMode};

/// Commands that can be sent to the graphic engine
pub enum GraphicCommand {
    /// Create a new window
    CreateWindow {
        /// Unique window ID assigned by GraphicProxy
        id: u64,
        /// Window configuration
        config: WindowConfig,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Close a window
    CloseWindow {
        /// Window ID to close
        id: u64,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set window size
    SetWindowSize {
        /// Window ID
        id: u64,
        /// New width in pixels
        width: u32,
        /// New height in pixels
        height: u32,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set window title
    SetWindowTitle {
        /// Window ID
        id: u64,
        /// New title
        title: String,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set window fullscreen mode
    SetWindowFullscreen {
        /// Window ID
        id: u64,
        /// Enable fullscreen
        fullscreen: bool,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set window visibility
    SetWindowVisible {
        /// Window ID
        id: u64,
        /// Show or hide
        visible: bool,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set window position
    SetWindowPosition {
        /// Window ID
        id: u64,
        /// X position in screen coordinates
        x: i32,
        /// Y position in screen coordinates
        y: i32,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set window position mode (centered, etc.)
    SetWindowPositionMode {
        /// Window ID
        id: u64,
        /// Position mode (see WindowPositionMode enum)
        mode: WindowPositionMode,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set window resizable property
    SetWindowResizable {
        /// Window ID
        id: u64,
        /// Whether the window should be resizable
        resizable: bool,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Get current mouse position (for sync requests outside frame loop)
    GetMousePosition {
        /// Window ID
        window_id: u64,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(f32, f32), String>>,
    },

    /// Check if a key is currently pressed
    IsKeyPressed {
        /// Key code to check
        key: String,
        /// Channel to send the result back
        response_tx: oneshot::Sender<bool>,
    },

    /// Get all currently pressed keys
    GetPressedKeys {
        /// Channel to send the result back
        response_tx: oneshot::Sender<Vec<String>>,
    },

    /// Shutdown the graphic engine gracefully
    Shutdown {
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },
}
