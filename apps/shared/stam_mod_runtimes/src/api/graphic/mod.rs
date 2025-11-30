//! Graphic Engine System
//!
//! This module provides a unified API for graphic engines (Bevy, WGPU, etc.)
//! that can be used by mods to create windows, handle input, and render graphics.
//!
//! The system is designed to be:
//! - Language-agnostic: Works with JavaScript, Lua, C#, and future runtimes
//! - Thread-safe: Engine runs in separate thread, communicates via channels
//! - Event-driven: Input and window events dispatched to registered handlers
//!
//! # Architecture
//!
//! ```text
//! Script Mods (JS/Lua/C#)
//!         |
//!         v
//! GraphicProxy (main thread, shared by all runtimes)
//!         |
//!    mpsc channels
//!         |
//!         v
//! GraphicEngine (separate thread - Bevy, WGPU, etc.)
//! ```

mod api;
mod engines;
mod proxy;
mod window;
mod command;
mod event;
mod input;

pub use api::GraphicApi;
pub use engines::{EngineFactory, GraphicEngine, GraphicEngines};
pub use proxy::{GraphicProxy, PendingMainThreadEngine};
pub use window::{WindowConfig, WindowInfo, WindowPositionMode};
pub use command::GraphicCommand;
pub use event::{GraphicEvent, KeyModifiers, MouseButton};
pub use input::{FrameSnapshot, MouseButtonState, GamepadState, KeyCode};
