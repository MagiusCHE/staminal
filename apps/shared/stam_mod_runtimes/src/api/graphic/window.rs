//! Window Configuration and State
//!
//! Defines window creation configuration and runtime state tracking.

/// Window position mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WindowPositionMode {
    /// Default position (OS decides)
    Default = 0,
    /// Center window on the primary monitor
    Centered = 1,
    /// Position at specific coordinates
    Manual = 2,
}

impl WindowPositionMode {
    /// Convert from u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Default),
            1 => Some(Self::Centered),
            2 => Some(Self::Manual),
            _ => None,
        }
    }
}

/// Configuration for creating a new window
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
    /// Allow window to be resized
    pub resizable: bool,
    /// Show window immediately after creation
    pub visible: bool,
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
        }
    }
}

impl WindowConfig {
    /// Create a new window config with custom title
    pub fn with_title(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    /// Set window size
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set fullscreen mode
    pub fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Set resizable flag
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set visibility
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

/// Runtime information about a window
#[derive(Clone, Debug)]
pub struct WindowInfo {
    /// Unique window ID
    pub id: u64,
    /// Current configuration/state
    pub config: WindowConfig,
    /// Whether the window has been created in the engine
    pub created: bool,
    /// Current position X (in screen coordinates)
    pub x: i32,
    /// Current position Y (in screen coordinates)
    pub y: i32,
    /// Whether the window is currently focused
    pub focused: bool,
}

impl WindowInfo {
    /// Create a new WindowInfo from a config
    pub fn new(id: u64, config: WindowConfig) -> Self {
        Self {
            id,
            config,
            created: false,
            x: 0,
            y: 0,
            focused: false,
        }
    }
}
