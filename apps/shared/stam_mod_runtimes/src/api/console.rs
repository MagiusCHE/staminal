/// Console API abstraction
///
/// Provides logging functionality that bridges to Rust's tracing system.
/// This module is runtime-agnostic and can be used by JavaScript, Lua, C#, etc.

use tracing::{info, error, debug, warn};

/// Console API implementation
#[derive(Clone)]
pub struct ConsoleApi;

impl ConsoleApi {
    /// Create a new ConsoleApi instance
    pub fn new() -> Self {
        Self
    }

    /// Log an info message
    pub fn log(runtime_type: &str, mod_id: &str, message: &str) {
        info!(runtime_type = runtime_type, mod_id = mod_id, "{}", message);
    }

    /// Log an error message
    pub fn error(runtime_type: &str, mod_id: &str, message: &str) {
        error!(runtime_type = runtime_type, mod_id = mod_id, "{}", message);
    }

    /// Log a warning message
    pub fn warn(runtime_type: &str, mod_id: &str, message: &str) {
        warn!(runtime_type = runtime_type, mod_id = mod_id, "{}", message);
    }

    /// Log an info message (alias for log)
    pub fn info(runtime_type: &str, mod_id: &str, message: &str) {
        info!(runtime_type = runtime_type, mod_id = mod_id, "{}", message);
    }

    /// Log a debug message
    pub fn debug(runtime_type: &str, mod_id: &str, message: &str) {
        debug!(runtime_type = runtime_type, mod_id = mod_id, "{}", message);
    }
}

impl Default for ConsoleApi {
    fn default() -> Self {
        Self::new()
    }
}
