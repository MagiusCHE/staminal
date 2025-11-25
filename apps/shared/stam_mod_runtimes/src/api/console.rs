/// Console API abstraction
///
/// Provides logging functionality that bridges to Rust's tracing system.
/// This module is runtime-agnostic and can be used by JavaScript, Lua, C#, etc.
///
/// The `game_id` (optional), `runtime_type` and `mod_id` fields are passed as tracing fields,
/// allowing custom formatters to display them as `game_id::js::mod-id` or `js::mod-id` format.

use tracing::{debug, error, info, warn};

/// Console API implementation
#[derive(Clone)]
pub struct ConsoleApi;

impl ConsoleApi {
    /// Create a new ConsoleApi instance
    pub fn new() -> Self {
        Self
    }

    /// Log an info message
    pub fn log(game_id: Option<&str>, runtime_type: &str, mod_id: &str, message: &str) {
        if let Some(gid) = game_id {
            info!(game_id = gid, runtime_type = runtime_type, mod_id = mod_id, message = message);
        } else {
            info!(runtime_type = runtime_type, mod_id = mod_id, message = message);
        }
    }

    /// Log an error message
    pub fn error(game_id: Option<&str>, runtime_type: &str, mod_id: &str, message: &str) {
        if let Some(gid) = game_id {
            error!(game_id = gid, runtime_type = runtime_type, mod_id = mod_id, message = message);
        } else {
            error!(runtime_type = runtime_type, mod_id = mod_id, message = message);
        }
    }

    /// Log a warning message
    pub fn warn(game_id: Option<&str>, runtime_type: &str, mod_id: &str, message: &str) {
        if let Some(gid) = game_id {
            warn!(game_id = gid, runtime_type = runtime_type, mod_id = mod_id, message = message);
        } else {
            warn!(runtime_type = runtime_type, mod_id = mod_id, message = message);
        }
    }

    /// Log an info message (alias for log)
    pub fn info(game_id: Option<&str>, runtime_type: &str, mod_id: &str, message: &str) {
        if let Some(gid) = game_id {
            info!(game_id = gid, runtime_type = runtime_type, mod_id = mod_id, message = message);
        } else {
            info!(runtime_type = runtime_type, mod_id = mod_id, message = message);
        }
    }

    /// Log a debug message
    pub fn debug(game_id: Option<&str>, runtime_type: &str, mod_id: &str, message: &str) {
        if let Some(gid) = game_id {
            debug!(game_id = gid, runtime_type = runtime_type, mod_id = mod_id, message = message);
        } else {
            debug!(runtime_type = runtime_type, mod_id = mod_id, message = message);
        }
    }
}

impl Default for ConsoleApi {
    fn default() -> Self {
        Self::new()
    }
}
