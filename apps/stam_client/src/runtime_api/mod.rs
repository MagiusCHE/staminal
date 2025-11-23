/// Runtime API abstraction layer
///
/// This module provides runtime-agnostic API implementations that can be
/// exposed to different scripting runtimes (JavaScript, Lua, C#, etc.).
///
/// Each API is implemented once in Rust and then bound to specific runtimes
/// through their respective binding layers.

pub mod console;
pub mod process;

pub use console::ConsoleApi;
pub use process::{ProcessApi, AppApi};
