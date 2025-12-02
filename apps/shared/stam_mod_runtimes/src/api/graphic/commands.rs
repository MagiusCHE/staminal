//! Graphic Commands
//!
//! Commands sent from the GraphicProxy (worker thread) to the graphic engine (main thread).

use super::{GraphicEngineInfo, PropertyValue, WidgetConfig, WidgetEventType, WidgetType, WindowConfig};
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

    // ========================================================================
    // Widget Commands
    // ========================================================================

    /// Create a new widget in a window
    CreateWidget {
        /// Parent window ID
        window_id: u64,
        /// Unique widget ID (assigned by GraphicProxy)
        widget_id: u64,
        /// Parent widget ID (None = root of window)
        parent_id: Option<u64>,
        /// Type of widget to create
        widget_type: WidgetType,
        /// Widget configuration
        config: WidgetConfig,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Update a widget property
    UpdateWidgetProperty {
        /// Widget ID to update
        widget_id: u64,
        /// Property name
        property: String,
        /// New property value
        value: PropertyValue,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Update multiple widget properties at once
    UpdateWidgetConfig {
        /// Widget ID to update
        widget_id: u64,
        /// New configuration (only set fields are updated)
        config: WidgetConfig,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Destroy a widget and all its children
    DestroyWidget {
        /// Widget ID to destroy
        widget_id: u64,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Move a widget to a new parent
    ReparentWidget {
        /// Widget ID to move
        widget_id: u64,
        /// New parent widget ID (None = root of window)
        new_parent_id: Option<u64>,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Destroy all widgets in a window
    ClearWindowWidgets {
        /// Window ID to clear
        window_id: u64,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Subscribe to widget events
    SubscribeWidgetEvents {
        /// Widget ID
        widget_id: u64,
        /// Event types to subscribe to
        event_types: Vec<WidgetEventType>,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Unsubscribe from widget events
    UnsubscribeWidgetEvents {
        /// Widget ID
        widget_id: u64,
        /// Event types to unsubscribe from
        event_types: Vec<WidgetEventType>,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    // ========================================================================
    // Asset Commands
    // ========================================================================

    /// Load a custom font
    LoadFont {
        /// Font file path (relative to mod/assets folder)
        path: String,
        /// Alias to use for this font (default: file name)
        alias: Option<String>,
        /// Channel to send the result back (returns assigned alias)
        response_tx: oneshot::Sender<Result<String, String>>,
    },

    /// Unload a font
    UnloadFont {
        /// Font alias to unload
        alias: String,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Preload an image for faster first use
    PreloadImage {
        /// Image file path
        path: String,
        /// Channel to send the result back
        response_tx: oneshot::Sender<Result<(), String>>,
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
            // Widget commands
            Self::CreateWidget {
                window_id,
                widget_id,
                parent_id,
                widget_type,
                ..
            } => f
                .debug_struct("CreateWidget")
                .field("window_id", window_id)
                .field("widget_id", widget_id)
                .field("parent_id", parent_id)
                .field("widget_type", widget_type)
                .finish(),
            Self::UpdateWidgetProperty {
                widget_id,
                property,
                value,
                ..
            } => f
                .debug_struct("UpdateWidgetProperty")
                .field("widget_id", widget_id)
                .field("property", property)
                .field("value", value)
                .finish(),
            Self::UpdateWidgetConfig { widget_id, .. } => f
                .debug_struct("UpdateWidgetConfig")
                .field("widget_id", widget_id)
                .finish(),
            Self::DestroyWidget { widget_id, .. } => f
                .debug_struct("DestroyWidget")
                .field("widget_id", widget_id)
                .finish(),
            Self::ReparentWidget {
                widget_id,
                new_parent_id,
                ..
            } => f
                .debug_struct("ReparentWidget")
                .field("widget_id", widget_id)
                .field("new_parent_id", new_parent_id)
                .finish(),
            Self::ClearWindowWidgets { window_id, .. } => f
                .debug_struct("ClearWindowWidgets")
                .field("window_id", window_id)
                .finish(),
            Self::SubscribeWidgetEvents {
                widget_id,
                event_types,
                ..
            } => f
                .debug_struct("SubscribeWidgetEvents")
                .field("widget_id", widget_id)
                .field("event_types", event_types)
                .finish(),
            Self::UnsubscribeWidgetEvents {
                widget_id,
                event_types,
                ..
            } => f
                .debug_struct("UnsubscribeWidgetEvents")
                .field("widget_id", widget_id)
                .field("event_types", event_types)
                .finish(),
            // Asset commands
            Self::LoadFont { path, alias, .. } => f
                .debug_struct("LoadFont")
                .field("path", path)
                .field("alias", alias)
                .finish(),
            Self::UnloadFont { alias, .. } => {
                f.debug_struct("UnloadFont").field("alias", alias).finish()
            }
            Self::PreloadImage { path, .. } => {
                f.debug_struct("PreloadImage").field("path", path).finish()
            }
        }
    }
}
