/// Mod Runtime System
///
/// This module re-exports the shared stam_mod_runtimes functionality
/// and provides client-specific configuration.

pub mod js_adapter;

// Re-export from shared stam_mod_runtimes
pub use stam_mod_runtimes::{RuntimeManager, RuntimeAdapter, RuntimeType, ModReturnValue};
pub use js_adapter::{JsRuntimeAdapter, JsRuntimeConfig};

// For backwards compatibility, keep ModRuntimeManager as an alias
pub type ModRuntimeManager = RuntimeManager;
