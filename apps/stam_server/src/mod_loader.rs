use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use tracing::info;

use stam_mod_runtimes::{
    RuntimeAdapter,
    adapters::js::{JsRuntimeAdapter, JsRuntimeConfig, register_mod_alias, has_fatal_error},
    api::{LocaleApi, ModInfo, SystemApi, UriResponse, ModPackagesRegistry},
    JsAsyncRuntime,
};
use stam_schema::{ModManifest, validate_mod_dependencies, Validatable};

use crate::config::{Config, GameConfig};

/// Runtime container for a single game's server-side mods
pub struct GameModRuntime {
    pub js_runtime: Option<Arc<JsAsyncRuntime>>,
    pub server_mods: Vec<String>,
    pub client_mods: Vec<String>,
    /// System API containing the event dispatcher for RequestUri handling
    pub system_api: Option<SystemApi>,
    /// Direct reference to the JS adapter for event dispatch
    /// Wrapped in Arc<RwLock> to allow async access from multiple handlers
    pub js_adapter: Option<Arc<RwLock<JsRuntimeAdapter>>>,
}

impl GameModRuntime {
    /// Dispatch a RequestUri event to registered handlers
    ///
    /// Returns a UriResponse with the result of handler processing
    pub async fn dispatch_request_uri(&self, uri: &str) -> UriResponse {
        if let Some(ref adapter) = self.js_adapter {
            let adapter = adapter.read().await;
            adapter.dispatch_request_uri(uri).await
        } else {
            // No JS adapter, return default 404 response
            UriResponse::default()
        }
    }

    /// Dispatch a TerminalKeyPressed event to registered handlers
    ///
    /// Returns a TerminalKeyResponse indicating whether the event was handled
    pub async fn dispatch_terminal_key(&self, request: &stam_mod_runtimes::api::TerminalKeyRequest) -> stam_mod_runtimes::api::TerminalKeyResponse {
        if let Some(ref adapter) = self.js_adapter {
            let adapter = adapter.read().await;
            adapter.dispatch_terminal_key(request).await
        } else {
            // No JS adapter, return default unhandled response
            stam_mod_runtimes::api::TerminalKeyResponse::default()
        }
    }

    /// Dispatch a custom event to registered handlers
    ///
    /// Returns a CustomEventResponse containing whether the event was handled
    /// and any custom properties set by handlers.
    ///
    /// **IMPORTANT**: Handler response values must be set SYNCHRONOUSLY before any
    /// `await` points. Values set after an `await` will not be captured.
    pub async fn dispatch_custom_event(&self, request: &stam_mod_runtimes::api::CustomEventRequest) -> stam_mod_runtimes::api::CustomEventResponse {
        if let Some(ref adapter) = self.js_adapter {
            let adapter = adapter.read().await;
            adapter.dispatch_custom_event(request).await
        } else {
            // No JS adapter, return default unhandled response
            stam_mod_runtimes::api::CustomEventResponse::default()
        }
    }

    /// Take the send_event request receiver from EventDispatcher (if available)
    ///
    /// This is used by the main loop to receive and process send_event requests
    /// from JavaScript mods calling `system.sendEvent()`.
    pub async fn take_send_event_receiver(&self) -> Option<tokio::sync::mpsc::Receiver<stam_mod_runtimes::api::SendEventRequest>> {
        if let Some(ref system_api) = self.system_api {
            system_api.event_dispatcher().take_send_event_receiver().await
        } else {
            None
        }
    }

    /// Get the home directory path from the system API
    pub fn get_home_dir(&self) -> Option<PathBuf> {
        self.system_api.as_ref().and_then(|api| api.get_home_dir())
    }

    /// Take the shutdown request receiver from SystemApi (if available)
    ///
    /// This is used by the main loop to receive shutdown requests from mods.
    pub async fn take_shutdown_receiver(&self) -> Option<tokio::sync::mpsc::Receiver<stam_mod_runtimes::api::ShutdownRequest>> {
        if let Some(ref system_api) = self.system_api {
            system_api.take_shutdown_receiver().await
        } else {
            None
        }
    }

    /// Get the number of handlers registered for TerminalKeyPressed event
    ///
    /// This is used to determine if any mod has registered to handle terminal input,
    /// which affects whether the default "Ctrl+C to exit" message should be shown.
    pub async fn terminal_key_handler_count(&self) -> usize {
        if let Some(ref adapter) = self.js_adapter {
            let adapter = adapter.read().await;
            adapter.terminal_key_handler_count()
        } else {
            0
        }
    }
}

/// Initialize mods for all games defined in configuration.
/// Validates dependencies for both client and server mods (skipping @client on server),
/// then loads and attaches server-side mods.
pub fn initialize_all_games(
    config: &Config,
    server_version: &str,
    custom_home: Option<&str>,
) -> Result<HashMap<String, GameModRuntime>, String> {
    let mods_root = resolve_mods_root(&config.mods_path, custom_home)?;

    // Determine home directory for mod-packages.json
    let home_dir = if let Some(home) = custom_home {
        PathBuf::from(home)
    } else {
        env::current_dir()
            .map_err(|e| format!("Failed to get current directory: {}", e))?
    };

    // Collect all enabled mod IDs from enabled games
    let enabled_mod_ids: std::collections::HashSet<String> = config.games.iter()
        .filter(|(_, game_config)| game_config.enabled)
        .flat_map(|(_, game_config)| {
            game_config.mods.iter()
                .filter(|(_, mod_config)| mod_config.enabled)
                .map(|(mod_id, _)| mod_id.clone())
        })
        .collect();

    // Load mod packages registry from STAM_HOME/mod-packages/mod-packages.json
    // and filter to only include packages for enabled mods
    let full_registry = ModPackagesRegistry::load_from_home(&home_dir)
        .map_err(|e| format!("Failed to load mod-packages.json: {}", e))?;

    let mod_packages = ModPackagesRegistry {
        client: full_registry.client.into_iter()
            .filter(|pkg| enabled_mod_ids.contains(&pkg.id))
            .collect(),
        server: full_registry.server.into_iter()
            .filter(|pkg| enabled_mod_ids.contains(&pkg.id))
            .collect(),
    };

    info!(
        "Loaded mod-packages: {} client packages, {} server packages (from {} enabled mods)",
        mod_packages.client.len(),
        mod_packages.server.len(),
        enabled_mod_ids.len()
    );

    let mut runtimes: HashMap<String, GameModRuntime> = HashMap::new();

    for (game_id, game_config) in &config.games {
        // Skip disabled games
        if !game_config.enabled {
            info!("Skipping disabled game '{}'", game_id);
            continue;
        }

        let game_runtime = initialize_game_mods(game_id, game_config, &mods_root, server_version, &home_dir, &mod_packages)?;
        runtimes.insert(game_id.clone(), game_runtime);
    }

    Ok(runtimes)
}

fn initialize_game_mods(
    game_id: &str,
    game_config: &GameConfig,
    mods_root: &std::path::Path,
    server_version: &str,
    home_dir: &std::path::Path,
    mod_packages: &ModPackagesRegistry,
) -> Result<GameModRuntime, String> {
    // Load manifests for all enabled mods first (per side based on execute_on from manifest)
    let mut client_manifests: HashMap<String, ModManifest> = HashMap::new();
    let mut server_manifests: HashMap<String, ModManifest> = HashMap::new();
    let mut client_mods: Vec<String> = Vec::new();
    let mut server_mods: Vec<String> = Vec::new();
    let mut server_manifest_dirs: HashMap<String, PathBuf> = HashMap::new();
    info!("> Initializing mods for game '{}'", game_id);
    for (mod_id, mod_cfg) in &game_config.mods {
        if !mod_cfg.enabled {
            continue;
        }

        let mod_dir = mods_root.join(mod_id);

        // execute_on is populated from manifest during config validation
        let is_client_mod = mod_cfg.execute_on.contains("client");
        let is_server_mod = mod_cfg.execute_on.contains("server");

        // Client manifest resolution
        if is_client_mod {
            client_mods.push(mod_id.clone());
            let (client_manifest, _client_base) =
                resolve_manifest(game_id, mod_id, &mod_dir, Some("client"))?;
            client_manifests.insert(mod_id.clone(), client_manifest);
        }

        // Server manifest resolution
        if is_server_mod {
            server_mods.push(mod_id.clone());
            let (server_manifest, server_base_dir) =
                resolve_manifest(game_id, mod_id, &mod_dir, Some("server"))?;
            server_manifests.insert(mod_id.clone(), server_manifest);
            server_manifest_dirs.insert(mod_id.clone(), server_base_dir);
        }
    }

    // Validate dependencies for client-side manifests (skip @client)
    for mod_id in &client_mods {
        if let Some(manifest) = client_manifests.get(mod_id) {
            let skip_client_requirement = true;
            validate_mod_dependencies(
                mod_id,
                manifest,
                &client_manifests,
                server_version, // not used when skipping @client
                &game_config.version,
                server_version,
                skip_client_requirement,
            )?;
        }
    }

    // Validate dependencies for server-side manifests (skip @client)
    for mod_id in &server_mods {
        if let Some(manifest) = server_manifests.get(mod_id) {
            let skip_client_requirement = true;
            validate_mod_dependencies(
                mod_id,
                manifest,
                &server_manifests,
                server_version, // not used when skipping @client
                &game_config.version,
                server_version,
                skip_client_requirement,
            )?;
        }
    }

    // Prepare JS adapter (only if we have server mods)
    let mut js_runtime_handle: Option<Arc<JsAsyncRuntime>> = None;
    let mut system_api_ref = None;
    let mut js_adapter_ref: Option<Arc<RwLock<JsRuntimeAdapter>>> = None;

    if !server_mods.is_empty() {
        let (data_dir, config_dir) = server_runtime_paths(game_id)?;
        let js_config = JsRuntimeConfig::new(data_dir, config_dir)
            .with_game_id(game_id);
        let mut js_adapter = JsRuntimeAdapter::new(js_config)
            .map_err(|e| format!("Game '{}': Failed to initialize JS runtime: {}", game_id, e))?;
        js_runtime_handle = Some(js_adapter.get_runtime());

        // Setup locale API for server-side mods (using stub fallback)
        // Server-side mods can have their own locale/ directories for translations
        // The global fallback returns message IDs in brackets since server has no global locale
        let locale_api = LocaleApi::new(
            "en-US",  // default locale
            "en-US",  // fallback locale
            |id| format!("[{}]", id),  // global fallback: return ID in brackets
            |id, _args| format!("[{}]", id),  // global fallback with args
        );
        js_adapter.set_locale_api(locale_api);

        // Set mod packages registry and home directory for system.get_mod_packages()
        js_adapter.system_api().set_mod_packages(mod_packages.clone());
        js_adapter.system_api().set_home_dir(home_dir.to_path_buf());

        // First pass: register aliases and mod info for all server mods
        // Mods without entry_point are asset-only and automatically considered attached
        let mut mod_entries: Vec<(String, PathBuf, String)> = Vec::new();
        for mod_id in &server_mods {
            let manifest = server_manifests.get(mod_id).ok_or_else(|| {
                format!("Game '{}': Missing manifest for mod '{}'", game_id, mod_id)
            })?;

            let base_dir = server_manifest_dirs.get(mod_id).cloned().unwrap_or_else(|| mods_root.join(mod_id));

            // Check if mod has an entry_point
            if let Some(ref entry_point) = manifest.entry_point {
                let entry_point_path = base_dir.join(entry_point);
                let absolute_entry_point = if entry_point_path.is_absolute() {
                    entry_point_path.clone()
                } else {
                    std::env::current_dir()
                        .map_err(|e| format!("Cannot resolve current directory: {}", e))?
                        .join(&entry_point_path)
                };

                register_mod_alias(mod_id, absolute_entry_point);

                // Register mod info with the system API
                // Server loads all mods immediately, so loaded: true
                // download_url is None on server (mods are already local)
                // exists: true on server (all mods are local)
                // archive fields are None on server (not needed)
                js_adapter.register_mod_info(ModInfo {
                    id: mod_id.clone(),
                    version: manifest.version.clone(),
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                    mod_type: manifest.mod_type.clone(),
                    priority: manifest.priority,
                    bootstrapped: false,
                    loaded: true,
                    exists: true,
                    download_url: None,
                    archive_sha512: None,
                    archive_bytes: None,
                    uncompressed_bytes: None,
                });

                mod_entries.push((mod_id.clone(), entry_point_path, manifest.mod_type.clone().unwrap_or_default()));
            } else {
                // Asset-only mod (no entry_point) - automatically considered attached
                info!("  - Mod '{}' has no entry_point, registering as asset-only (auto-attached)", mod_id);
                js_adapter.register_mod_info(ModInfo {
                    id: mod_id.clone(),
                    version: manifest.version.clone(),
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                    mod_type: manifest.mod_type.clone(),
                    priority: manifest.priority,
                    bootstrapped: false, // No code to bootstrap
                    loaded: false,       // This cannot be "loaded" since no code is present
                    exists: true,
                    download_url: None,
                    archive_sha512: None,
                    archive_bytes: None,
                    uncompressed_bytes: None,
                });
                // Don't add to mod_entries - no code to load/attach
            }
        }

        // Store reference to system API for setting bootstrapped state later
        system_api_ref = Some(js_adapter.system_api().clone());

        // Second pass: load mods and call onAttach
        info!("  - Attaching server mods for game '{}'", game_id);
        for (mod_id, entry_point_path, _mod_type) in &mod_entries {
            js_adapter
                .load_mod(&entry_point_path, mod_id)
                .map_err(|e| format!("{}::{} Failed to load mod: {}", game_id, mod_id, e))?;
            js_adapter
                .call_mod_function(mod_id, "onAttach")
                .map_err(|e| format!("{}::{} Failed to call onAttach: {}", game_id, mod_id, e))?;

            // Check for fatal JS errors (unhandled promise rejections) after each attach
            // This catches async errors that occur during onAttach
            if has_fatal_error() {
                return Err(format!(
                    "{}::{} Fatal JavaScript error during onAttach",
                    game_id, mod_id
                ));
            }
            //debug!("Attached '{}'", mod_id);
        }

        // Third pass: call onBootstrap for bootstrap mods
        let bootstrap_mods: Vec<_> = mod_entries
            .iter()
            .filter(|(_, _, mod_type)| mod_type.eq_ignore_ascii_case("bootstrap") || mod_type.eq_ignore_ascii_case("boostrap"))
            .collect();

        if !bootstrap_mods.is_empty() {
            info!("  - Bootstrapping server mods for game '{}'", game_id);
            for (mod_id, _, _) in &bootstrap_mods {
                js_adapter
                    .call_mod_function(mod_id, "onBootstrap")
                    .map_err(|e| format!("{}::{} Failed to call onBootstrap: {}", game_id, mod_id, e))?;

                // Check for fatal JS errors after each bootstrap
                if has_fatal_error() {
                    return Err(format!(
                        "{}::{} Fatal JavaScript error during onBootstrap",
                        game_id, mod_id
                    ));
                }

                // Mark mod as bootstrapped
                if let Some(ref system_api) = system_api_ref {
                    system_api.set_bootstrapped(mod_id, true);
                }
                //debug!("Bootstrapped '{}'", mod_id);
            }
        }

        // Wrap adapter in Arc<RwLock> for shared access AFTER initialization is complete
        // This avoids blocking calls within the async runtime
        js_adapter_ref = Some(Arc::new(RwLock::new(js_adapter)));
    }

    info!(
        "< Initialization complete for game '{}' (server_mods={}, client_mods={})",
        game_id,
        server_mods.len(),
        client_mods.len(),
    );

    Ok(GameModRuntime {
        js_runtime: js_runtime_handle,
        server_mods,
        client_mods,
        system_api: system_api_ref,
        js_adapter: js_adapter_ref,
    })
}

fn server_runtime_paths(game_id: &str) -> Result<(PathBuf, PathBuf), String> {
    let base = PathBuf::from("apps/stam_server/workspace_data/runtime").join(game_id);
    let data_dir = base.join("data");
    let config_dir = base.join("config");

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create data dir for game '{}': {}", game_id, e))?;
    std::fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config dir for game '{}': {}", game_id, e))?;

    Ok((data_dir, config_dir))
}

fn load_manifest(game_id: &str, mod_id: &str, path: &PathBuf) -> Result<(ModManifest, PathBuf), String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| format!("Invalid manifest path for mod '{}' in game '{}'", mod_id, game_id))?;
    let manifest = ModManifest::from_json_file(path_str)
        .map_err(|e| format!("Game '{}': Failed to load manifest for mod '{}': {}", game_id, mod_id, e))?;
    let base_dir = path.parent()
        .map(PathBuf::from)
        .ok_or_else(|| format!("Cannot determine base dir for manifest of mod '{}' in game '{}'", mod_id, game_id))?;
    Ok((manifest, base_dir))
}

fn resolve_mods_root(mods_path: &str, custom_home: Option<&str>) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(mods_path);
    if candidate.is_absolute() {
        return Ok(candidate);
    }

    if let Some(home) = custom_home {
        return Ok(PathBuf::from(home).join(candidate));
    }

    let cwd = env::current_dir()
        .map_err(|e| format!("Failed to get current directory: {}", e))?;
    Ok(cwd.join(candidate))
}

/// Resolve manifest path according to folder rules:
/// - If a side-specific subfolder exists (e.g., "client" or "server") and contains manifest.json, use it.
/// - Otherwise, fall back to the root manifest.json.
fn resolve_manifest(
    game_id: &str,
    mod_id: &str,
    mod_dir: &PathBuf,
    side_folder: Option<&str>,
) -> Result<(ModManifest, PathBuf), String> {
    if let Some(side) = side_folder {
        let side_dir = mod_dir.join(side);
        let side_manifest = side_dir.join("manifest.json");
        if side_manifest.exists() {
            return load_manifest(game_id, mod_id, &side_manifest);
        }
        // If side dir exists but manifest is missing, still fall back to root manifest if present
    }

    let root_manifest = mod_dir.join("manifest.json");
    if root_manifest.exists() {
        return load_manifest(game_id, mod_id, &root_manifest);
    }

    Err(format!(
        "Game '{}': Mod '{}' missing manifest (checked {} and {})",
        game_id,
        mod_id,
        side_folder
            .map(|s| mod_dir.join(s).join("manifest.json").display().to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        root_manifest.display()
    ))
}
