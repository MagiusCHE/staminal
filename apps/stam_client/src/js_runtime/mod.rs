/// JavaScript runtime integration for Staminal client
///
/// This module provides QuickJS runtime integration for executing JavaScript mods.
/// Future runtimes (V8, etc.) can be added here as alternatives.

mod console_api;
mod runtime;

// Re-export the main runtime
pub use runtime::JsRuntime;

// Future: expose runtime selection
// pub enum RuntimeType {
//     QuickJS,
//     V8,
// }
