/// JavaScript runtime integration for Staminal client
///
/// This module provides QuickJS runtime integration for executing JavaScript mods.
/// Future runtimes (V8, etc.) can be added here as alternatives.

mod console_api;
mod process_api;
mod runtime;
mod config;

// Re-export the main runtime and config
pub use runtime::JsRuntime;
pub use config::ScriptRuntimeConfig;

// Future: expose runtime selection
// pub enum RuntimeType {
//     QuickJS,
//     V8,
// }
