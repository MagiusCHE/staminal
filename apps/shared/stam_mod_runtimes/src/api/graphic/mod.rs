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

mod commands;
pub mod ecs;
mod engines;
mod events;
mod proxy;
mod widget;
mod window;

pub use commands::GraphicCommand;
pub use engines::{GraphicEngine, GraphicEngineInfo, GraphicEngines};
pub use events::{GraphicEvent, KeyModifiers, MouseButton};
pub use proxy::{EnableEngineRequest, GraphicProxy};
pub use widget::{
    AlignItems, BlendMode, ColorParseError, ColorValue, EdgeInsets, FlexDirection, FontConfig,
    FontInfo, FontStyle, FontWeight, ImageConfig, ImageScaleMode, ImageSource, JustifyContent, LayoutType,
    PropertyValue, RectValue, ShadowConfig, SizeValue, TextAlign, WidgetConfig, WidgetEventType,
    WidgetFilter, WidgetInfo, WidgetSubscriptions, WidgetType,
};
pub use window::{InitialWindowConfig, WindowConfig, WindowInfo, WindowMode, WindowPositionMode};
