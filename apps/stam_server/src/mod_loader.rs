use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use tracing::info;

use stam_mod_runtimes::{
    RuntimeManager,
    RuntimeType,
    adapters::js::{JsRuntimeAdapter, JsRuntimeConfig, register_mod_alias},
    api::{LocaleApi, ModInfo},
    JsAsyncRuntime,
};
use stam_schema::{ModManifest, validate_mod_dependencies, Validatable};

use crate::config::{Config, GameConfig};

/// Runtime container for a single game's server-side mods
pub struct GameModRuntime {
    pub runtime_manager: RuntimeManager,
    pub js_runtime: Option<Arc<JsAsyncRuntime>>,
    pub server_mods: Vec<String>,
    pub client_mods: Vec<String>,
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
    let mut runtimes: HashMap<String, GameModRuntime> = HashMap::new();

    for (game_id, game_config) in &config.games {        
        let game_runtime = initialize_game_mods(game_id, game_config, &mods_root, server_version)?;
        runtimes.insert(game_id.clone(), game_runtime);
    }

    Ok(runtimes)
}

fn initialize_game_mods(
    game_id: &str,
    game_config: &GameConfig,
    mods_root: &std::path::Path,
    server_version: &str,
) -> Result<GameModRuntime, String> {
    // Load manifests for all enabled mods first (per side)
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

        // Client manifest resolution
        if mod_cfg.side.iter().any(|s| s == "client") {
            client_mods.push(mod_id.clone());
            let (client_manifest, _client_base) =
                resolve_manifest(game_id, mod_id, &mod_dir, Some("client"))?;
            client_manifests.insert(mod_id.clone(), client_manifest);
        }

        // Server manifest resolution
        if mod_cfg.side.iter().any(|s| s == "server") {
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

    // Prepare runtime manager and JS adapter (only if we have server mods)
    let mut runtime_manager = RuntimeManager::new();
    let mut js_runtime_handle: Option<Arc<JsAsyncRuntime>> = None;
    let mut system_api_ref = None;

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

        // First pass: register aliases and mod info for all server mods
        let mut mod_entries: Vec<(String, PathBuf, String)> = Vec::new();
        for mod_id in &server_mods {
            let manifest = server_manifests.get(mod_id).ok_or_else(|| {
                format!("Game '{}': Missing manifest for mod '{}'", game_id, mod_id)
            })?;

            let base_dir = server_manifest_dirs.get(mod_id).cloned().unwrap_or_else(|| mods_root.join(mod_id));
            let entry_point_path = base_dir.join(&manifest.entry_point);
            let absolute_entry_point = if entry_point_path.is_absolute() {
                entry_point_path.clone()
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("Cannot resolve current directory: {}", e))?
                    .join(&entry_point_path)
            };

            register_mod_alias(mod_id, absolute_entry_point);

            // Register mod info with the system API (before adapter is boxed)
            // Server loads all mods immediately, so loaded: true
            js_adapter.register_mod_info(ModInfo {
                id: mod_id.clone(),
                version: manifest.version.clone(),
                name: manifest.name.clone(),
                description: manifest.description.clone(),
                mod_type: manifest.mod_type.clone(),
                priority: manifest.priority,
                bootstrapped: false,
                loaded: true,
            });

            mod_entries.push((mod_id.clone(), entry_point_path, manifest.mod_type.clone().unwrap_or_default()));
        }

        // Store reference to system API for setting bootstrapped state later
        system_api_ref = Some(js_adapter.system_api().clone());

        // Now register the adapter with the runtime manager
        runtime_manager.register_adapter(RuntimeType::JavaScript, Box::new(js_adapter));

        // Second pass: load mods and call onAttach
        info!("  - Attaching server mods for game '{}'", game_id);
        for (mod_id, entry_point_path, _mod_type) in &mod_entries {
            runtime_manager
                .load_mod(mod_id, entry_point_path)
                .map_err(|e| format!("{}::{} Failed to load mod: {}", game_id, mod_id, e))?;
            runtime_manager
                .call_mod_function(mod_id, "onAttach")
                .map_err(|e| format!("{}::{} Failed to call onAttach: {}", game_id, mod_id, e))?;
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
                runtime_manager
                    .call_mod_function(mod_id, "onBootstrap")
                    .map_err(|e| format!("{}::{} Failed to call onBootstrap: {}", game_id, mod_id, e))?;
                // Mark mod as bootstrapped
                if let Some(ref system_api) = system_api_ref {
                    system_api.set_bootstrapped(mod_id, true);
                }
                //debug!("Bootstrapped '{}'", mod_id);
            }
        }
    }

    info!(
        "< Initialization complete for game '{}' (server_mods={}, client_mods={})",
        game_id,
        server_mods.len(),
        client_mods.len(),
    );

    Ok(GameModRuntime {
        runtime_manager,
        js_runtime: js_runtime_handle,
        server_mods,
        client_mods,
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
