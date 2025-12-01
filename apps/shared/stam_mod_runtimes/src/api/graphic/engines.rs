//! Graphic Engine Types and Traits
//!
//! Defines the available graphic engines and the trait that all engines must implement.

use super::{GraphicCommand, InitialWindowConfig};
use serde::Serialize;

/// Information about a graphic engine
///
/// This struct contains metadata about the active graphic engine,
/// including its type, version, and capabilities.
#[derive(Debug, Clone, Serialize)]
pub struct GraphicEngineInfo {
    /// The engine type (Bevy, Wgpu, Terminal)
    pub engine_type: String,
    /// The engine type as numeric ID
    pub engine_type_id: u32,
    /// The library/engine name
    pub name: String,
    /// The library version
    pub version: String,
    /// Detailed description of the engine
    pub description: String,
    /// List of supported features
    pub features: Vec<String>,
    /// The rendering backend being used (e.g., "Vulkan", "Metal", "DX12", "WebGPU")
    pub backend: String,
    /// Whether the engine supports 2D rendering
    pub supports_2d: bool,
    /// Whether the engine supports 3D rendering
    pub supports_3d: bool,
    /// Whether the engine supports UI rendering
    pub supports_ui: bool,
    /// Whether the engine supports audio
    pub supports_audio: bool,
}

/// Available graphic engines
///
/// Each engine provides different capabilities and trade-offs:
/// - **Bevy**: Full-featured game engine with ECS, 2D/3D rendering, audio, etc.
/// - **Wgpu**: Low-level GPU access for custom rendering (future)
/// - **Terminal**: Text-based interface for CLI games (future)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum GraphicEngines {
    /// Bevy game engine - full-featured, ECS-based
    Bevy = 0,
    /// Raw WGPU - for custom rendering (future)
    Wgpu = 1,
    /// Terminal/TUI mode - for text-based interfaces (future)
    Terminal = 2,
}

impl GraphicEngines {
    /// Convert from u32 to GraphicEngines
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Bevy),
            1 => Some(Self::Wgpu),
            2 => Some(Self::Terminal),
            _ => None,
        }
    }

    /// Convert to u32
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    /// Get the engine name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bevy => "Bevy",
            Self::Wgpu => "Wgpu",
            Self::Terminal => "Terminal",
        }
    }

    /// Check if the engine is currently supported
    pub fn is_supported(&self) -> bool {
        match self {
            Self::Bevy => true,
            Self::Wgpu => false,     // Future
            Self::Terminal => false, // Future
        }
    }
}

/// Trait for graphic engine implementations
///
/// Each engine (Bevy, WGPU, etc.) implements this trait to provide
/// a consistent interface for the GraphicProxy.
///
/// # Threading
///
/// The `run` method is called on the main thread and blocks until
/// shutdown is requested. This is required because many windowing
/// systems (especially on macOS) require window management on the main thread.
pub trait GraphicEngine: Send + 'static {
    /// Initialize the engine and start its main loop
    ///
    /// This method runs on the main thread and should not return
    /// until shutdown is requested via the command channel.
    ///
    /// # Arguments
    /// * `command_rx` - Channel to receive commands from the proxy
    /// * `initial_window_config` - Optional configuration for the initial/main window
    fn run(
        &mut self,
        command_rx: std::sync::mpsc::Receiver<GraphicCommand>,
        initial_window_config: Option<InitialWindowConfig>,
    );

    /// Get the engine type
    fn engine_type(&self) -> GraphicEngines;

    /// Get detailed information about the engine
    ///
    /// Returns a `GraphicEngineInfo` struct with engine metadata,
    /// version information, and capability flags.
    fn get_engine_info(&self) -> GraphicEngineInfo;
}
