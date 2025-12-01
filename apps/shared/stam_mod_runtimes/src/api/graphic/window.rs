//! Window Configuration and State
//!
//! Types for window creation and runtime window information.

/// Window position mode
///
/// Controls how the window is positioned on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPositionMode {
    /// Use default/automatic positioning
    Default,
    /// Center the window on the primary monitor
    Centered,
    /// Position at specific coordinates
    At(i32, i32),
}

impl WindowPositionMode {
    /// Convert from u32 enum value
    ///
    /// 0 = Default, 1 = Centered, 2+ = reserved for At (requires separate x,y params)
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::Default,
            1 => Self::Centered,
            _ => Self::Default,
        }
    }

    /// Convert to u32 enum value
    pub fn to_u32(&self) -> u32 {
        match self {
            Self::Default => 0,
            Self::Centered => 1,
            Self::At(_, _) => 2,
        }
    }
}

/// Configuration for creating a new window
///
/// This struct is passed to `GraphicProxy::create_window()` and forwarded
/// to the engine thread for window creation.
///
/// This config uses the same fields as `InitialWindowConfig` (used in `enableEngine`)
/// to ensure consistency between the two APIs.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Window title
    pub title: String,
    /// Initial width in pixels
    pub width: u32,
    /// Initial height in pixels
    pub height: u32,
    /// Start in fullscreen mode
    pub fullscreen: bool,
    /// Allow window resizing
    pub resizable: bool,
    /// Window is visible on creation
    pub visible: bool,
    /// Window position mode (applied at creation time)
    pub position_mode: WindowPositionMode,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Staminal".to_string(),
            width: 1280,
            height: 720,
            fullscreen: false,
            resizable: true,
            visible: true,
            position_mode: WindowPositionMode::Centered,
        }
    }
}

impl WindowConfig {
    /// Create a new WindowConfig with the given title
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    /// Set the window size
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set fullscreen mode
    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Set resizable flag
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set visibility
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

/// Initial window configuration for engine startup
///
/// This struct is passed to `GraphicProxy::enable_engine()` to configure
/// the main window that is created when the engine starts.
/// Unlike runtime window modifications, these settings are applied at creation time,
/// which is important for features like positioning that may not work after creation
/// (e.g., on Wayland).
#[derive(Clone, Debug)]
pub struct InitialWindowConfig {
    /// Window title
    pub title: String,
    /// Initial width in pixels
    pub width: u32,
    /// Initial height in pixels
    pub height: u32,
    /// Allow window resizing
    pub resizable: bool,
    /// Start in fullscreen mode
    pub fullscreen: bool,
    /// Window position mode (applied at creation time)
    pub position_mode: WindowPositionMode,
}

impl Default for InitialWindowConfig {
    fn default() -> Self {
        Self {
            title: "Staminal".to_string(),
            width: 1280,
            height: 720,
            resizable: true,
            fullscreen: false,
            position_mode: WindowPositionMode::Centered,
        }
    }
}

/// Runtime information about a window
///
/// This struct tracks the current state of a window managed by the graphic engine.
#[derive(Clone, Debug)]
pub struct WindowInfo {
    /// Unique window identifier
    pub id: u64,
    /// Current window configuration
    pub config: WindowConfig,
    /// Whether the window has been successfully created in the engine
    pub created: bool,
}

impl WindowInfo {
    /// Create a new WindowInfo
    pub fn new(id: u64, config: WindowConfig) -> Self {
        Self {
            id,
            config,
            created: false,
        }
    }

    /// Mark the window as created
    pub fn mark_created(&mut self) {
        self.created = true;
    }
}
