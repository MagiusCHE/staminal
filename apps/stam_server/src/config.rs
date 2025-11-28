use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use stam_schema::{ModManifest, Validatable, StringOrArray};
use stam_protocol::ModInfo;
use std::collections::HashMap;
use std::path::Path;

/// Mod configuration for a game
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModConfig {
    /// Whether this mod is enabled
    pub enabled: bool,
    /// Mod type (e.g., "bootstrap", "library") - read from manifest at validation time
    #[serde(rename = "type", skip_serializing_if = "Option::is_none", default)]
    pub mod_type: Option<String>,
    /// URI for client to download this mod
    #[serde(default)]
    pub client_download: String,
    /// Which side(s) this mod applies to - populated from manifest's execute_on at validation time
    #[serde(skip)]
    pub execute_on: StringOrArray,
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
    /// Reads mod_type and execute_on from each mod's manifest.json file
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
            // First pass: read manifests and populate mod_type and execute_on for each enabled mod
            let mod_ids: Vec<String> = game_config.mods.keys().cloned().collect();

            for mod_id in &mod_ids {
                let mod_config = game_config.mods.get_mut(mod_id).unwrap();

                if !mod_config.enabled {
                    continue; // Skip disabled mods
                }

                let mod_dir = mods_path.join(mod_id);

                // Collect execute_on from all available manifests
                // A mod can have: root manifest.json, server/manifest.json, client/manifest.json
                // We need to check all of them to determine full execute_on scope
                let mut combined_execute_on: Vec<String> = Vec::new();
                let mut primary_manifest: Option<ModManifest> = None;

                // Check root manifest.json
                let root_manifest_path = mod_dir.join("manifest.json");
                if root_manifest_path.exists() {
                    let manifest = ModManifest::from_json_file(root_manifest_path.to_str().unwrap_or(""))
                        .map_err(|e| format!(
                            "Game '{}': Failed to read root manifest for mod '{}': {}",
                            game_id, mod_id, e
                        ))?;
                    for platform in manifest.execute_on.iter() {
                        if !combined_execute_on.contains(platform) {
                            combined_execute_on.push(platform.clone());
                        }
                    }
                    primary_manifest = Some(manifest);
                }

                // Check server/manifest.json
                let server_manifest_path = mod_dir.join("server").join("manifest.json");
                if server_manifest_path.exists() {
                    let manifest = ModManifest::from_json_file(server_manifest_path.to_str().unwrap_or(""))
                        .map_err(|e| format!(
                            "Game '{}': Failed to read server manifest for mod '{}': {}",
                            game_id, mod_id, e
                        ))?;
                    for platform in manifest.execute_on.iter() {
                        if !combined_execute_on.contains(platform) {
                            combined_execute_on.push(platform.clone());
                        }
                    }
                    if primary_manifest.is_none() {
                        primary_manifest = Some(manifest);
                    }
                }

                // Check client/manifest.json
                let client_manifest_path = mod_dir.join("client").join("manifest.json");
                if client_manifest_path.exists() {
                    let manifest = ModManifest::from_json_file(client_manifest_path.to_str().unwrap_or(""))
                        .map_err(|e| format!(
                            "Game '{}': Failed to read client manifest for mod '{}': {}",
                            game_id, mod_id, e
                        ))?;
                    for platform in manifest.execute_on.iter() {
                        if !combined_execute_on.contains(platform) {
                            combined_execute_on.push(platform.clone());
                        }
                    }
                    if primary_manifest.is_none() {
                        primary_manifest = Some(manifest);
                    }
                }

                // Ensure we found at least one manifest
                let manifest = primary_manifest.ok_or_else(|| format!(
                    "Game '{}': No manifest.json found for mod '{}'",
                    game_id, mod_id
                ))?;

                // Populate execute_on from combined manifests
                mod_config.execute_on = StringOrArray(combined_execute_on);

                // Populate mod_type from manifest if not set in config
                if mod_config.mod_type.is_none() {
                    mod_config.mod_type = manifest.mod_type;
                }

                let is_client_mod = mod_config.execute_on.contains("client");
                let is_server_mod = mod_config.execute_on.contains("server");

                if !is_client_mod && !is_server_mod {
                    return Err(format!(
                        "Game '{}': Mod '{}' must declare execute_on with at least one of 'client' or 'server' in manifest",
                        game_id, mod_id
                    ));
                }

                // Validate mod_type is set (from manifest or config)
                if mod_config.mod_type.is_none() {
                    return Err(format!(
                        "Game '{}': Mod '{}' has no 'type' field in manifest",
                        game_id, mod_id
                    ));
                }

                // Replace placeholders in client_download
                // {{public_uri}} -> server's public_uri
                // {{mod_id}} -> current mod's id
                if !mod_config.client_download.is_empty() {
                    let public_uri = self.public_uri.as_deref().unwrap_or("");
                    mod_config.client_download = mod_config.client_download
                        .replace("{{public_uri}}", public_uri)
                        .replace("{{mod_id}}", mod_id);

                    // Normalize URL: replace multiple slashes with single slash (except after scheme)
                    // e.g., stam://host:port//path -> stam://host:port/path
                    if let Some(scheme_end) = mod_config.client_download.find("://") {
                        let (scheme, rest) = mod_config.client_download.split_at(scheme_end + 3);
                        // Split by / and rejoin, preserving the leading slash for the path
                        if let Some(first_slash) = rest.find('/') {
                            let (host_port, path) = rest.split_at(first_slash);
                            let normalized_path = path.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/");
                            mod_config.client_download = format!("{}{}/{}", scheme, host_port, normalized_path);
                        }
                    }
                }

                // Validate client_download is not empty for client mods
                if is_client_mod && mod_config.client_download.is_empty() {
                    return Err(format!(
                        "Game '{}': Mod '{}' has empty 'client_download' field (required for client mods)",
                        game_id, mod_id
                    ));
                }
            }

            // Build mod list for this game (done once at boot)
            game_config.mod_list = game_config.mods.iter()
                .filter(|(_, mod_config)| mod_config.enabled)
                .filter(|(_, mod_config)| mod_config.execute_on.contains("client"))
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
