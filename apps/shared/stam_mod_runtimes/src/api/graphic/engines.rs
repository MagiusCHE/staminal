//! Graphic Engine Types and Trait
//!
//! Defines the available graphic engines and the common trait they must implement.

use std::sync::Arc;
use tokio::sync::mpsc;
use super::command::GraphicCommand;
use super::event::GraphicEvent;

/// Type alias for an engine factory function
///
/// The factory is called when a script requests `system.enable_graphic_engine(type)`.
/// It should return a boxed engine instance that will be run in a separate thread.
pub type EngineFactory = Arc<dyn Fn() -> Box<dyn GraphicEngine> + Send + Sync>;

/// Available graphic engines
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
    /// Get engine name as string
    pub fn name(&self) -> &'static str {
        match self {
            GraphicEngines::Bevy => "Bevy",
            GraphicEngines::Wgpu => "WGPU",
            GraphicEngines::Terminal => "Terminal",
        }
    }

    /// Try to convert from u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(GraphicEngines::Bevy),
            1 => Some(GraphicEngines::Wgpu),
            2 => Some(GraphicEngines::Terminal),
            _ => None,
        }
    }
}

/// Trait for graphic engine implementations
///
/// Each engine (Bevy, WGPU, etc.) implements this trait to provide
/// a consistent interface for the GraphicProxy.
pub trait GraphicEngine: Send + 'static {
    /// Initialize the engine and start its main loop
    ///
    /// This method runs on the engine thread and should not return
    /// until shutdown is requested.
    ///
    /// # Arguments
    /// * `command_rx` - Channel to receive commands from the main thread
    /// * `event_tx` - Channel to send events back to the main thread
    fn run(
        &mut self,
        command_rx: mpsc::Receiver<GraphicCommand>,
        event_tx: mpsc::Sender<GraphicEvent>,
    );

    /// Get the engine type
    fn engine_type(&self) -> GraphicEngines;

    /// Whether this engine requires running on the main thread
    ///
    /// Some engines (like Bevy with winit on Linux/Wayland) require
    /// running on the main thread for proper window event handling.
    /// When this returns true, the engine's run() method will be called
    /// on the main thread instead of a separate thread.
    ///
    /// Default: false (engine runs in a separate thread)
    fn require_main_thread(&self) -> bool {
        false
    }
}
