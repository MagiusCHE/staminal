use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use stam_schema::Validatable;
use std::collections::HashMap;

/// Mod configuration for a game
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModConfig {
    /// Whether this mod is enabled
    pub enabled: bool,
    /// URI for client to download this mod (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_download: Option<String>,
}

/// Game configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GameConfig {
    /// Game name (human-readable)
    pub name: String,
    /// Game version
    pub version: String,
    /// Mods configuration for this game
    #[serde(default)]
    pub mods: HashMap<String, ModConfig>,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "Staminal Server Configuration")]
#[schemars(description = "Configuration for Staminal Core Server")]
pub struct Config {
    /// Server name
    #[serde(default = "default_name")]
    #[schemars(description = "Human-readable server name displayed to clients")]
    pub name: String,

    /// Local IP address to bind the server
    #[serde(default = "default_local_ip")]
    #[schemars(description = "IP address to bind the server (e.g., '0.0.0.0' for all interfaces)")]
    pub local_ip: String,

    /// Local port number
    #[serde(default = "default_local_port")]
    #[schemars(description = "Port number for the game server", range(min = 1024, max = 65535))]
    pub local_port: u16,

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

    /// Public URI for server list (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Public URI advertised in server list (e.g., 'stam://game.example.com:9999')")]
    pub public_uri: Option<String>,

    /// Games configuration
    #[serde(default)]
    #[schemars(description = "Available games on this server")]
    pub games: HashMap<String, GameConfig>,
}

fn default_name() -> String {
    "Staminal Server".to_string()
}

fn default_local_ip() -> String {
    "0.0.0.0".to_string()
}

fn default_local_port() -> u16 {
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
            name: default_name(),
            local_ip: default_local_ip(),
            local_port: default_local_port(),
            log_level: default_log_level(),
            mods_path: default_mods_path(),
            tick_rate: default_tick_rate(),
            public_uri: None,
            games: HashMap::new(),
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
        assert_eq!(config.local_ip, "0.0.0.0");
        assert_eq!(config.local_port, 7777);
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn test_valid_json() {
        let json = r#"{
            "local_ip": "127.0.0.1",
            "local_port": 8080,
            "log_level": "debug",
            "mods_path": "./mods",
            "tick_rate": 30
        }"#;

        let config = Config::from_json_str(json).unwrap();
        assert_eq!(config.local_ip, "127.0.0.1");
        assert_eq!(config.local_port, 8080);
        assert_eq!(config.log_level, "debug");
    }

    #[test]
    fn test_invalid_log_level() {
        let json = r#"{
            "local_ip": "127.0.0.1",
            "local_port": 8080,
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
            "local_ip": "127.0.0.1",
            "local_port": 99999,
            "log_level": "info",
            "mods_path": "./mods",
            "tick_rate": 30
        }"#;

        let result = Config::from_json_str(json);
        assert!(result.is_err());
    }
}
