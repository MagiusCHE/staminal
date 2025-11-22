use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use stam_schema::Validatable;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "Staminal Server Configuration")]
#[schemars(description = "Configuration for Staminal Core Server")]
pub struct Config {
    /// Host address to bind the server
    #[serde(default = "default_host")]
    #[schemars(description = "IP address to bind the server (e.g., '0.0.0.0' for all interfaces)")]
    pub host: String,

    /// UDP port number
    #[serde(default = "default_port")]
    #[schemars(description = "UDP port number for the game server", range(min = 1024, max = 65535))]
    pub port: u16,

    /// Logging level
    #[serde(default = "default_log_level")]
    #[schemars(description = "Log level: trace, debug, info, warn, error")]
    #[schemars(regex(pattern = r"^(trace|debug|info|warn|error)$"))]
    pub log_level: String,

    /// Path to mods directory
    #[serde(default = "default_mods_path")]
    #[schemars(description = "Directory path where mod files are located")]
    pub mods_path: String,

    /// Server tick rate in Hz
    #[serde(default = "default_tick_rate")]
    #[schemars(description = "Server update frequency in ticks per second", range(min = 1, max = 1000))]
    pub tick_rate: u64,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    7777
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_mods_path() -> String {
    "../mods".to_string()
}

fn default_tick_rate() -> u64 {
    64
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            log_level: default_log_level(),
            mods_path: default_mods_path(),
            tick_rate: default_tick_rate(),
        }
    }
}

// Implement Validatable for Config
impl Validatable for Config {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 7777);
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn test_valid_json() {
        let json = r#"{
            "host": "127.0.0.1",
            "port": 8080,
            "log_level": "debug",
            "mods_path": "./mods",
            "tick_rate": 30
        }"#;

        let config = Config::from_json_str(json).unwrap();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn test_invalid_log_level() {
        let json = r#"{
            "host": "127.0.0.1",
            "port": 8080,
            "log_level": "invalid",
            "mods_path": "./mods",
            "tick_rate": 30
        }"#;

        let result = Config::from_json_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_port() {
        let json = r#"{
            "host": "127.0.0.1",
            "port": 99999,
            "log_level": "info",
            "mods_path": "./mods",
            "tick_rate": 30
        }"#;

        let result = Config::from_json_str(json);
        assert!(result.is_err());
    }
}
