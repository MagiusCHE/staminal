//! Graphic Engine API
//!
//! Language-agnostic API for graphic engine operations.
//! Shared by all scripting runtimes (JS, Lua, C#, etc.)
//!
//! # Architecture
//!
//! The graphic system uses a proxy pattern to communicate between the main thread
//! (where graphic engines like Bevy run) and the worker thread (where scripts execute).
//!
//! ```text
//! +------------------+     +------------------+     +------------------+
//! |   Script (JS)    | --> |   GraphicProxy   | --> |   Bevy Engine    |
//! |   Worker Thread  |     |   (channels)     |     |   Main Thread    |
//! +------------------+     +------------------+     +------------------+
//! ```
//!
//! # Client-Only
//!
//! All graphic operations are client-only. On the server, `GraphicProxy` returns
//! descriptive errors for all operations.
//!
//! # UI System
//!
//! The UI system uses the ECS API for creating UI elements. The legacy widget system
//! has been removed. See `ecs` module and `docs/graphic/ecs.md` for more information.

mod commands;
pub mod ecs;
mod engines;
mod events;
mod proxy;
mod common_types;
mod window;

pub use commands::GraphicCommand;
pub use engines::{GraphicEngine, GraphicEngineInfo, GraphicEngines};
pub use events::{GraphicEvent, KeyModifiers, MouseButton};
pub use proxy::{EnableEngineRequest, GraphicProxy};
pub use common_types::{
    AlignItems, BlendMode, ColorParseError, ColorValue, EdgeInsets, FlexDirection, FontConfig,
    FontInfo, FontStyle, FontWeight, ImageConfig, ImageScaleMode, ImageSource, JustifyContent, LayoutType,
    RectValue, ShadowConfig, SizeValue, TextAlign,
};
pub use window::{InitialWindowConfig, WindowConfig, WindowInfo, WindowMode, WindowPositionMode};
