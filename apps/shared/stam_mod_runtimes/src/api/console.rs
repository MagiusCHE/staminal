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
    pub fn log(mod_id: &str, message: &str) {
        info!("\"{}\" {}", mod_id, message);
    }

    /// Log an error message
    pub fn error(mod_id: &str, message: &str) {
        error!("\"{}\" {}", mod_id, message);
    }

    /// Log a warning message
    pub fn warn(mod_id: &str, message: &str) {
        warn!("\"{}\" {}", mod_id, message);
    }

    /// Log an info message (alias for log)
    pub fn info(mod_id: &str, message: &str) {
        info!("\"{}\" {}", mod_id, message);
    }

    /// Log a debug message
    pub fn debug(mod_id: &str, message: &str) {
        debug!("\"{}\" {}", mod_id, message);
    }
}

impl Default for ConsoleApi {
    fn default() -> Self {
        Self::new()
    }
}
