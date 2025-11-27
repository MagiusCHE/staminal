use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use stam_schema::{ModManifest, Validatable};
use stam_protocol::ModInfo;
use std::collections::HashMap;
use std::path::Path;

/// Mod configuration for a game
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModConfig {
    /// Whether this mod is enabled
    pub enabled: bool,
    /// Mod type (e.g., "bootstrap", "library") - read from manifest, stored here after validation
    #[serde(rename = "type", skip_serializing_if = "Option::is_none", default)]
    pub mod_type: Option<String>,
    /// URI for client to download this mod
    #[serde(default)]
    pub client_download: String,
    /// Which side(s) this mod applies to ("client", "server")
    #[serde(default)]
    pub side: Vec<String>,
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
    /// Pre-built mod list (not serialized, built at runtime)
    #[serde(skip)]
    pub mod_list: Vec<ModInfo>,
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
    "./mods".to_string()
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

impl Config {
    /// Validate the configuration and build mod lists for all games
    /// Reads mod_type from each mod's manifest.json file
    /// Returns an error if any game has mods with missing required fields
    pub fn validate_mods(&mut self, custom_home: Option<&str>) -> Result<(), String> {
        // Resolve mods path similar to mod_loader::resolve_mods_root
        let mods_path = {
            let candidate = Path::new(&self.mods_path);
            if candidate.is_absolute() {
                candidate.to_path_buf()
            } else if let Some(home) = custom_home {
                Path::new(home).join(candidate)
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("Failed to get current directory: {}", e))?
                    .join(candidate)
            }
        };

        for (game_id, game_config) in &mut self.games {
            // First pass: read manifests and populate mod_type for each enabled mod
            let mod_ids: Vec<String> = game_config.mods.keys().cloned().collect();

            for mod_id in &mod_ids {
                let mod_config = game_config.mods.get_mut(mod_id).unwrap();

                if !mod_config.enabled {
                    continue; // Skip disabled mods
                }

                let is_client_mod = mod_config.side.iter().any(|s| s == "client");
                let is_server_mod = mod_config.side.iter().any(|s| s == "server");

                if !is_client_mod && !is_server_mod {
                    return Err(format!(
                        "Game '{}': Mod '{}' must declare at least one side ('client' or 'server')",
                        game_id, mod_id
                    ));
                }

                // Read mod_type from manifest if not already set in config
                // Use same resolution logic as mod_loader: check side-specific folder first, then root
                if mod_config.mod_type.is_none() {
                    let mod_dir = mods_path.join(mod_id);

                    // Determine which side to check for manifest
                    // For server-side mods, prefer server/manifest.json
                    // For client-only mods, prefer client/manifest.json
                    let side_folder = if is_server_mod { "server" } else { "client" };

                    // Try side-specific manifest first, then fall back to root
                    let manifest_path = {
                        let side_manifest = mod_dir.join(side_folder).join("manifest.json");
                        if side_manifest.exists() {
                            side_manifest
                        } else {
                            mod_dir.join("manifest.json")
                        }
                    };

                    let manifest = ModManifest::from_json_file(manifest_path.to_str().unwrap_or(""))
                        .map_err(|e| format!(
                            "Game '{}': Failed to read manifest for mod '{}': {}",
                            game_id, mod_id, e
                        ))?;

                    mod_config.mod_type = manifest.mod_type;
                }

                // Validate mod_type is set (from manifest or config)
                if mod_config.mod_type.is_none() {
                    return Err(format!(
                        "Game '{}': Mod '{}' has no 'type' field in manifest",
                        game_id, mod_id
                    ));
                }

                // Validate client_download is not empty for client mods
                if is_client_mod && mod_config.client_download.is_empty() {
                    return Err(format!(
                        "Game '{}': Mod '{}' has empty 'client_download' field",
                        game_id, mod_id
                    ));
                }
            }

            // Build mod list for this game (done once at boot)
            game_config.mod_list = game_config.mods.iter()
                .filter(|(_, mod_config)| mod_config.enabled)
                .filter(|(_, mod_config)| mod_config.side.iter().any(|s| s == "client"))
                .map(|(mod_id, mod_config)| {
                    ModInfo {
                        mod_id: mod_id.clone(),
                        mod_type: mod_config.mod_type.clone().unwrap_or_default(),
                        download_url: mod_config.client_download.clone(),
                    }
                })
                .collect();
        }

        Ok(())
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
