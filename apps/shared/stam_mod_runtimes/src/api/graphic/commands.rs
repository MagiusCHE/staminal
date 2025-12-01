//! Graphic Commands
//!
//! Commands sent from the GraphicProxy (worker thread) to the graphic engine (main thread).

use super::{GraphicEngineInfo, WindowConfig};
use tokio::sync::oneshot;

/// Commands that can be sent to the graphic engine
///
/// Each command includes a `response_tx` channel for the engine to send
/// back the result. This enables async/await patterns in the calling code.
///
/// # Threading
///
/// Commands are sent via `std::sync::mpsc` channel from the worker thread
/// to the main thread where the graphic engine runs.
pub enum GraphicCommand {
    /// Create a new window
    CreateWindow {
        /// Unique window ID (assigned by GraphicProxy)
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

    /// Update window size
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

    /// Update window title
    SetWindowTitle {
        /// Window ID
        id: u64,
        /// New title
        title: String,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Set fullscreen mode
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
        /// Make visible
        visible: bool,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    // Note: SetWindowResizable was removed - resizable must be set at window creation time

    /// Shutdown the graphic engine
    ///
    /// The engine should:
    /// 1. Stop accepting new commands
    /// 2. Close all open windows gracefully
    /// 3. Release all GPU/rendering resources
    /// 4. Send Ok(()) response
    /// 5. Exit its main loop
    Shutdown {
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Get engine information
    ///
    /// Returns metadata about the graphic engine including
    /// version, capabilities, and rendering backend.
    GetEngineInfo {
        /// Channel to send the engine info back
        response_tx: oneshot::Sender<GraphicEngineInfo>,
    },
}

impl std::fmt::Debug for GraphicCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateWindow { id, config, .. } => f
                .debug_struct("CreateWindow")
                .field("id", id)
                .field("config", config)
                .finish(),
            Self::CloseWindow { id, .. } => {
                f.debug_struct("CloseWindow").field("id", id).finish()
            }
            Self::SetWindowSize {
                id, width, height, ..
            } => f
                .debug_struct("SetWindowSize")
                .field("id", id)
                .field("width", width)
                .field("height", height)
                .finish(),
            Self::SetWindowTitle { id, title, .. } => f
                .debug_struct("SetWindowTitle")
                .field("id", id)
                .field("title", title)
                .finish(),
            Self::SetWindowFullscreen { id, fullscreen, .. } => f
                .debug_struct("SetWindowFullscreen")
                .field("id", id)
                .field("fullscreen", fullscreen)
                .finish(),
            Self::SetWindowVisible { id, visible, .. } => f
                .debug_struct("SetWindowVisible")
                .field("id", id)
                .field("visible", visible)
                .finish(),
            Self::Shutdown { .. } => f.debug_struct("Shutdown").finish(),
            Self::GetEngineInfo { .. } => f.debug_struct("GetEngineInfo").finish(),
        }
    }
}
