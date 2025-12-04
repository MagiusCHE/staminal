use clap::Parser;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use tokio::net::TcpStream;
use tracing::{Level, debug, error, info, trace, warn};

use stam_mod_runtimes::api::{
    DownloadResponse, EnableEngineRequest, GraphicCommand, GraphicEngineReadyRequest,
    GraphicEngineWindowClosedRequest, GraphicEngines, GraphicEvent, GraphicProxy, LocaleApi,
    NetworkApi, NetworkConfig, extract_mod_archive, parse_stam_uri, sanitize_uri,
};
use stam_log::{LogConfig, init_logging};
use stam_protocol::{GameMessage, GameStream, IntentType, PrimalMessage, PrimalStream};
use stam_schema::{ModManifest, Validatable, validate_mod_dependencies, validate_version_range};

mod engines;
use engines::BevyEngine;

// ============================================================================
// Worker Thread Communication
// ============================================================================

/// Messages sent from the worker thread to the main thread
#[derive(Debug)]
enum WorkerMessage {
    /// Worker thread terminated normally with an exit code
    Terminated { exit_code: i32 },
    /// Worker thread encountered a fatal error
    Error { message: String },
}

/// Messages sent from the main thread to the worker thread
#[derive(Debug)]
#[allow(dead_code)] // Will be used when GraphicEngine is implemented
enum MainMessage {
    /// Request graceful shutdown of the worker
    Shutdown,
}

#[macro_use]
mod locale;
use locale::LocaleManager;

mod app_paths;
mod mod_runtime;

use app_paths::AppPaths;
use mod_runtime::js_adapter::{create_js_runtime_config, run_js_event_loop};
use mod_runtime::{JsRuntimeAdapter, JsRuntimeConfig, ModInfo, ModRuntimeManager};

const VERSION: &str = "0.1.0";

/// Compute SHA-512 hash of a string and return as hex string
fn sha512_hash(input: &str) -> String {
    let mut hasher = Sha512::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Perform a stam:// URI request and return the response
///
/// This function creates a new TCP connection to the server, performs
/// the RequestUri protocol exchange, and returns the response.
///
/// If `tmp_dir` is provided and the response contains file content, the content
/// will be saved to a temp file and `temp_file_path` will be set in the response.
/// The `file_content` field will be cleared to avoid memory duplication.
///
/// # Arguments
/// * `uri` - The stam:// URI to request
/// * `username` - Default username if not in URI
/// * `password_hash` - Default password hash if not in URI
/// * `game_id` - The game ID for the request
/// * `client_version` - Client version string
/// * `default_server` - Default server address (host:port) to use if URI has no host
/// * `tmp_dir` - Optional temp directory for saving file downloads
/// * `progress_callback` - Optional callback for progress updates (percentage, received, total)
async fn perform_stam_request(
    uri: &str,
    username: &str,
    password_hash: &str,
    game_id: &str,
    client_version: &str,
    default_server: &str,
    tmp_dir: Option<&std::path::Path>,
    progress_callback: Option<stam_mod_runtimes::api::ProgressCallback>,
) -> DownloadResponse {
    // Parse the URI to extract host:port
    let (mut host_port, path, uri_username, uri_password) = match parse_stam_uri(uri) {
        Some(parsed) => parsed,
        None => {
            error!("Invalid stam:// URI: {}", uri);
            return DownloadResponse {
                status: 400,
                buffer_string: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            };
        }
    };

    // If URI has no host, use the default server
    if host_port.is_empty() {
        host_port = default_server.to_string();
    }

    // Use credentials from URI if provided, otherwise use default
    let effective_username = uri_username.as_ref().map(|s| s.as_str()).unwrap_or(username);
    let effective_password_hash = if let Some(pwd) = uri_password {
        sha512_hash(&pwd)
    } else {
        password_hash.to_string()
    };

    // Sanitize URI (remove credentials) for sending to server
    let sanitized_uri = sanitize_uri(uri);

    debug!("Performing stam:// request: host={}, path={}", host_port, path);

    // Connect to server
    let mut stream = match TcpStream::connect(&host_port).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to connect to {}: {}", host_port, e);
            return DownloadResponse {
                status: 503, // Service Unavailable
                buffer_string: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            };
        }
    };

    // Read Welcome message
    match stream.read_primal_message().await {
        Ok(PrimalMessage::Welcome { version: _ }) => {
            // Version check could be done here, but for now we just proceed
        }
        Ok(msg) => {
            error!("Unexpected message during RequestUri: {:?}", msg);
            return DownloadResponse {
                status: 500,
                buffer_string: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            };
        }
        Err(e) => {
            error!("Failed to read Welcome during RequestUri: {}", e);
            return DownloadResponse {
                status: 500,
                buffer_string: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            };
        }
    }

    // Send RequestUri Intent
    let intent = PrimalMessage::Intent {
        intent_type: IntentType::RequestUri,
        client_version: client_version.to_string(),
        username: effective_username.to_string(),
        password_hash: effective_password_hash,
        game_id: Some(game_id.to_string()),
        uri: Some(sanitized_uri),
    };

    if let Err(e) = stream.write_primal_message(&intent).await {
        error!("Failed to send RequestUri Intent: {}", e);
        return DownloadResponse {
            status: 500,
            buffer_string: None,
            file_name: None,
            file_content: None,
            temp_file_path: None,
        };
    }

    // Wait for UriResponse header
    match stream.read_primal_message().await {
        Ok(PrimalMessage::UriResponse { status, buffer_string, file_name, file_size }) => {
            debug!("Received UriResponse: status={}, file_name={:?}, file_size={:?}, buffer_string_len={:?}",
                status, file_name, file_size, buffer_string.as_ref().map(|s| s.len()));

            // If buffer_string is present in response, this is a non-chunked transfer (small data or simple response)
            if let Some(content_string) = buffer_string {
                // Convert string to bytes for file operations
                let content_bytes = content_string.as_bytes().to_vec();

                // Data was sent directly in the response
                if file_name.is_some() && tmp_dir.is_some() {
                    // Save to temp file
                    let tmp_dir = tmp_dir.unwrap();
                    let temp_file_name = generate_temp_filename(file_name.as_deref());
                    let temp_path = tmp_dir.join(&temp_file_name);

                    if !tmp_dir.exists() {
                        if let Err(e) = std::fs::create_dir_all(tmp_dir) {
                            error!("Failed to create temp directory: {}", e);
                            return DownloadResponse {
                                status,
                                buffer_string: Some(content_string.clone()),
                                file_name,
                                file_content: Some(content_bytes.clone()),
                                temp_file_path: None,
                            };
                        }
                    }

                    match std::fs::write(&temp_path, &content_bytes) {
                        Ok(_) => {
                            return DownloadResponse {
                                status,
                                buffer_string: Some(content_string),
                                file_name,
                                file_content: None,
                                temp_file_path: Some(temp_path.to_string_lossy().to_string()),
                            };
                        }
                        Err(e) => {
                            error!("Failed to write temp file: {}", e);
                            return DownloadResponse {
                                status,
                                buffer_string: Some(content_string),
                                file_name,
                                file_content: Some(content_bytes),
                                temp_file_path: None,
                            };
                        }
                    }
                } else if file_name.is_some() {
                    // Return as file_content
                    return DownloadResponse {
                        status,
                        buffer_string: Some(content_string.clone()),
                        file_name,
                        file_content: Some(content_bytes),
                        temp_file_path: None,
                    };
                } else {
                    // Return as buffer_string
                    return DownloadResponse {
                        status,
                        buffer_string: Some(content_string),
                        file_name: None,
                        file_content: None,
                        temp_file_path: None,
                    };
                }
            }

            // No buffer means chunked transfer - read chunks until final
            // Use raw chunk reading for zero-copy performance
            let total_size = file_size.unwrap_or(0);
            let mut received_bytes: u64 = 0;

            // Pre-allocate the final buffer with exact size (or grow as needed)
            let mut all_data: Vec<u8> = if total_size > 0 {
                vec![0u8; total_size as usize]  // Allocate with zeros, we'll write directly into it
            } else {
                Vec::new()
            };

            // Temporary chunk buffer for unknown-size transfers
            let mut chunk_buffer: Vec<u8> = if total_size == 0 {
                vec![0u8; 16 * 1024 * 1024]  // 16MB default chunk buffer
            } else {
                Vec::new()
            };

            loop {
                if total_size > 0 {
                    // Known size: read directly into final buffer position
                    let offset = received_bytes as usize;

                    match stream.read_raw_chunk(&mut all_data[offset..]).await {
                        Ok((bytes_read, is_final)) => {
                            received_bytes += bytes_read as u64;

                            // Call progress callback if provided
                            if let Some(ref callback) = progress_callback {
                                let percentage = (received_bytes as f64 / total_size as f64) * 100.0;
                                callback(percentage, received_bytes, total_size);
                                // Yield to allow other tasks to run (UI updates, input handling)
                                tokio::task::yield_now().await;
                            }

                            if is_final {
                                // Truncate to actual size received
                                all_data.truncate(received_bytes as usize);
                                debug!("Received final chunk, total {} bytes", received_bytes);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to read raw chunk: {}", e);
                            return DownloadResponse {
                                status: 500,
                                buffer_string: None,
                                file_name: None,
                                file_content: None,
                                temp_file_path: None,
                            };
                        }
                    }
                } else {
                    // Unknown size: read into temp buffer then append
                    match stream.read_raw_chunk(&mut chunk_buffer).await {
                        Ok((bytes_read, is_final)) => {
                            received_bytes += bytes_read as u64;
                            all_data.extend_from_slice(&chunk_buffer[..bytes_read]);

                            // Call progress callback if provided
                            if let Some(ref callback) = progress_callback {
                                callback(0.0, received_bytes, 0);
                                tokio::task::yield_now().await;
                            }

                            if is_final {
                                debug!("Received final chunk, total {} bytes", received_bytes);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to read raw chunk: {}", e);
                            return DownloadResponse {
                                status: 500,
                                buffer_string: None,
                                file_name: None,
                                file_content: None,
                                temp_file_path: None,
                            };
                        }
                    }
                }
            }

            // Save to temp file if tmp_dir is provided and file_name is present
            if file_name.is_some() && tmp_dir.is_some() {
                let tmp_dir = tmp_dir.unwrap();
                let temp_file_name = generate_temp_filename(file_name.as_deref());
                let temp_path = tmp_dir.join(&temp_file_name);

                if !tmp_dir.exists() {
                    if let Err(e) = std::fs::create_dir_all(tmp_dir) {
                        error!("Failed to create temp directory: {}", e);
                        return DownloadResponse {
                            status,
                            buffer_string: None,
                            file_name,
                            file_content: Some(all_data),
                            temp_file_path: None,
                        };
                    }
                }

                match std::fs::write(&temp_path, &all_data) {
                    Ok(_) => {
                        DownloadResponse {
                            status,
                            buffer_string: None,
                            file_name,
                            file_content: None,
                            temp_file_path: Some(temp_path.to_string_lossy().to_string()),
                        }
                    }
                    Err(e) => {
                        error!("Failed to write temp file: {}", e);
                        DownloadResponse {
                            status,
                            buffer_string: None,
                            file_name,
                            file_content: Some(all_data),
                            temp_file_path: None,
                        }
                    }
                }
            } else if file_name.is_some() {
                // Return as file_content
                DownloadResponse {
                    status,
                    buffer_string: None,
                    file_name,
                    file_content: Some(all_data),
                    temp_file_path: None,
                }
            } else {
                // Return as buffer_string (convert bytes to UTF-8 string)
                let buffer_str = String::from_utf8_lossy(&all_data).to_string();
                DownloadResponse {
                    status,
                    buffer_string: Some(buffer_str),
                    file_name: None,
                    file_content: None,
                    temp_file_path: None,
                }
            }
        }
        Ok(PrimalMessage::Error { message }) => {
            error!("Server error during RequestUri: {}", message);
            DownloadResponse {
                status: 500,
                buffer_string: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            }
        }
        Ok(msg) => {
            error!("Unexpected response to RequestUri: {:?}", msg);
            DownloadResponse {
                status: 500,
                buffer_string: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            }
        }
        Err(e) => {
            error!("Failed to read UriResponse: {}", e);
            DownloadResponse {
                status: 500,
                buffer_string: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            }
        }
    }
}

/// Generate a unique temp filename with optional extension from original name
fn generate_temp_filename(original_name: Option<&str>) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let unique_id = std::process::id();

    if let Some(name) = original_name {
        let ext = std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("tmp");
        format!("download_{}_{}.{}", timestamp, unique_id, ext)
    } else {
        format!("download_{}_{}.tmp", timestamp, unique_id)
    }
}

/// Connect to game server and maintain connection
async fn connect_to_game_server(
    uri: &str,
    username: &str,
    password: &str,
    game_id: &str,
    locale: Arc<LocaleManager>,
    app_paths: &AppPaths,
    engine_request_tx: std_mpsc::Sender<EnableEngineRequest>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize game-specific directory
    debug!("Initializing directories for game '{}'...", game_id);
    let game_root = app_paths.game_root(game_id)?;
    let mods_dir = game_root.join("mods");

    debug!("Game directories:");
    debug!("  Root: {}", game_root.display());
    debug!("  Mods: {}", mods_dir.display());

    // Parse game server URI (stam://host:port)
    if !uri.starts_with("stam://") {
        return Err(locale
            .get_with_args(
                "error-invalid-uri",
                Some(&fluent_args! {
                    "uri" => uri
                }),
            )
            .into());
    }

    let host_port = uri.strip_prefix("stam://").unwrap();
    info!(
        "{}",
        locale.get_with_args(
            "game-connecting",
            Some(&fluent_args! {
                "host" => host_port
            })
        )
    );

    // Connect to game server
    let mut stream = TcpStream::connect(host_port).await?;
    info!("{}", locale.get("game-connected"));

    // Read Welcome message
    let mut server_version = String::new();

    match stream.read_primal_message().await {
        Ok(PrimalMessage::Welcome { version }) => {
            info!(
                "{}",
                locale.get_with_args(
                    "server-welcome",
                    Some(&fluent_args! {
                        "version" => version.as_str()
                    })
                )
            );

            // Check version compatibility
            let client_version_parts: Vec<&str> = VERSION.split('.').collect();
            let server_version_parts: Vec<&str> = version.split('.').collect();

            if client_version_parts.len() >= 2 && server_version_parts.len() >= 2 {
                let client_major_minor =
                    format!("{}.{}", client_version_parts[0], client_version_parts[1]);
                let server_major_minor =
                    format!("{}.{}", server_version_parts[0], server_version_parts[1]);

                if client_major_minor != server_major_minor {
                    error!(
                        "{}",
                        locale.get_with_args(
                            "version-mismatch",
                            Some(&fluent_args! {
                                "client" => VERSION,
                                "server" => version.as_str()
                            })
                        )
                    );
                    return Err(locale.get("disconnect-version-mismatch").into());
                }

                info!(
                    "{}",
                    locale.get_with_args(
                        "version-compatible",
                        Some(&fluent_args! {
                            "client" => VERSION,
                            "server" => version.as_str()
                        })
                    )
                );
            }

            server_version = version;
        }
        Ok(msg) => {
            error!("{}: {:?}", locale.get("error-unexpected-message"), msg);
            return Err(locale.get("error-unexpected-message").into());
        }
        Err(e) => {
            error!("{}: {}", locale.get("error-parse-failed"), e);
            return Err(e.into());
        }
    }

    // Send GameLogin Intent
    info!("{}", locale.get("login-sending"));
    let password_hash = sha512_hash(password);

    let intent = PrimalMessage::Intent {
        intent_type: IntentType::GameLogin,
        client_version: VERSION.to_string(),
        username: username.to_string(),
        password_hash: password_hash.clone(),
        game_id: Some(game_id.to_string()),
        uri: None,
    };

    stream.write_primal_message(&intent).await?;

    // Wait for LoginSuccess
    // JS runtime handle for event loop integration
    let mut js_runtime_handle: Option<std::sync::Arc<stam_mod_runtimes::JsAsyncRuntime>> = None;
    // Runtime manager and system API for dynamic mod loading
    let mut runtime_manager_opt: Option<ModRuntimeManager> = None;
    let mut system_api_opt: Option<stam_mod_runtimes::api::SystemApi> = None;
    // Graphic proxy for polling graphic engine events
    let mut graphic_proxy_opt: Option<Arc<GraphicProxy>> = None;
    // Game root directory for resolving mod paths
    let mut game_root_opt: Option<std::path::PathBuf> = None;

    match stream.read_game_message().await {
        Ok(GameMessage::LoginSuccess { game_name, game_version, mods }) => {
            info!("{} {} [{}]", locale.get("game-login-success"), game_name, game_version);
            let active_game_version = game_version.clone();

            // Log mod list received
            if !mods.is_empty() {
                debug!("Received {} required mod(s):", mods.len());
                // Print mods as indented JSON for debugging
                match serde_json::to_string_pretty(&mods) {
                    Ok(json) => debug!("Mods list:\n{}", json),
                    Err(e) => warn!("Failed to serialize mods list: {}", e),
                }
            } else {
                debug!("No mods required for this game");
            }

            // Load manifests only for mods that are present locally
            // Missing mods are tracked separately - we only fail if a required mod is missing
            // Tuple stores (manifest, actual_mod_dir) since mod_dir might be in client/ subdirectory
            let mut available_manifests: HashMap<String, (ModManifest, std::path::PathBuf)> = HashMap::new();
            let mut missing_mods: Vec<String> = Vec::new();

            if !mods.is_empty() {
                debug!("Server requires {} mod(s), checking local availability...", mods.len());

                // First pass: load manifests for available mods, track missing ones
                for mod_info in &mods {
                    let mod_dir = mods_dir.join(&mod_info.mod_id);

                    // Check if mod directory exists
                    if !mod_dir.exists() {
                        debug!("Mod '{}' not found locally", mod_info.mod_id);
                        missing_mods.push(mod_info.mod_id.clone());
                        continue;
                    }

                    // Read manifest - check client/ subdirectory first, then root
                    // If manifest is in client/, use that as the mod_dir for entry_point resolution
                    let client_dir = mod_dir.join("client");
                    let client_manifest_path = client_dir.join("manifest.json");
                    let root_manifest_path = mod_dir.join("manifest.json");

                    let (actual_mod_dir, manifest_path) = if client_manifest_path.exists() {
                        (client_dir, client_manifest_path)
                    } else if root_manifest_path.exists() {
                        (mod_dir.clone(), root_manifest_path)
                    } else {
                        warn!("Mod '{}' directory exists but missing manifest.json (checked client/ and root)", mod_info.mod_id);
                        missing_mods.push(mod_info.mod_id.clone());
                        continue;
                    };

                    let manifest = match ModManifest::from_json_file(manifest_path.to_str().unwrap()) {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("Failed to load manifest for mod '{}': {}", mod_info.mod_id, e);
                            missing_mods.push(mod_info.mod_id.clone());
                            continue;
                        }
                    };

                    debug!(" ✓ {} [{}:{}] found (from {})", mod_info.mod_id, mod_info.mod_type, manifest.version,
                        if actual_mod_dir != mod_dir { "client/" } else { "root" });
                    available_manifests.insert(mod_info.mod_id.clone(), (manifest, actual_mod_dir));
                }

                if !missing_mods.is_empty() {
                    debug!(" ? {} mod(s) not available locally: {:?}", missing_mods.len(), missing_mods);
                }
            } else {
                debug!("No mods required");
            }

            // CRITICAL: Check if any required bootstrap mod (or its dependencies) is missing
            // If so, download them from the server before proceeding
            // This loop continues until ALL bootstrap mods AND their recursive dependencies are downloaded
            let required_bootstrap_mods: Vec<&stam_protocol::ModInfo> = mods.iter()
                .filter(|m| m.mod_type == "bootstrap")
                .collect();

            if !required_bootstrap_mods.is_empty() {
                // Get tmp directory for downloads (once, outside the loop)
                // Use game-specific tmp directory: data_dir/{game_id}/tmp
                let tmp_dir = game_root.join("tmp");
                if !tmp_dir.exists() {
                    std::fs::create_dir_all(&tmp_dir)?;
                    debug!("Created game tmp directory: {}", tmp_dir.display());
                }

                // Keep downloading until all dependencies are satisfied
                let mut download_iteration = 0;
                const MAX_DOWNLOAD_ITERATIONS: u32 = 100; // Safety limit to prevent infinite loops

                loop {
                    download_iteration += 1;
                    if download_iteration > MAX_DOWNLOAD_ITERATIONS {
                        error!("FATAL: Too many download iterations ({}), possible circular dependency", download_iteration);
                        return Err("Download loop limit exceeded - possible circular dependency".into());
                    }

                    // Calculate all mods needed for bootstrap (bootstrap mods + their dependencies recursively)
                    // We need to use manifests from available mods to calculate dependencies
                    fn collect_required_mods_recursive(
                        mod_id: &str,
                        available_manifests: &HashMap<String, (ModManifest, std::path::PathBuf)>,
                        all_mods: &[stam_protocol::ModInfo],
                        required: &mut Vec<String>,
                        chain: &mut Vec<String>,
                    ) {
                        // Avoid infinite loops
                        if chain.contains(&mod_id.to_string()) || required.contains(&mod_id.to_string()) {
                            return;
                        }
                        chain.push(mod_id.to_string());

                        // If mod is available locally, check its dependencies from manifest
                        if let Some((manifest, _)) = available_manifests.get(mod_id) {
                            for (dep_id, _) in &manifest.requires {
                                if !dep_id.starts_with('@') {
                                    collect_required_mods_recursive(dep_id, available_manifests, all_mods, required, chain);
                                }
                            }
                        }
                        // Note: if mod is NOT available locally, we can't know its dependencies yet
                        // They will be resolved after download

                        chain.pop();
                        required.push(mod_id.to_string());
                    }

                    let mut mods_required_for_bootstrap: Vec<String> = Vec::new();
                    let mut chain: Vec<String> = Vec::new();

                    for bootstrap_mod in &required_bootstrap_mods {
                        collect_required_mods_recursive(
                            &bootstrap_mod.mod_id,
                            &available_manifests,
                            &mods,
                            &mut mods_required_for_bootstrap,
                            &mut chain,
                        );
                    }

                    // Find which of the required mods are missing
                    let mods_to_download: Vec<&stam_protocol::ModInfo> = mods_required_for_bootstrap.iter()
                        .filter(|mod_id| !available_manifests.contains_key(*mod_id))
                        .filter_map(|mod_id| mods.iter().find(|m| &m.mod_id == mod_id))
                        .collect();

                    // If nothing to download, we're done!
                    if mods_to_download.is_empty() {
                        if download_iteration > 1 {
                            debug!("All bootstrap mods and dependencies downloaded after {} iteration(s)", download_iteration - 1);
                        }
                        break;
                    }

                    debug!("[Iteration {}] Need to download {} mod(s) for bootstrap: {:?}",
                        download_iteration,
                        mods_to_download.len(),
                        mods_to_download.iter().map(|m| &m.mod_id).collect::<Vec<_>>());

                    // Download each missing mod
                    for mod_info in &mods_to_download {
                        if mod_info.download_url.is_empty() {
                            error!("FATAL: Mod '{}' has no download URL", mod_info.mod_id);
                            return Err(format!(
                                "Cannot download mod '{}': no download URL provided by server",
                                mod_info.mod_id
                            ).into());
                        }

                        debug!("Downloading mod '{}' from {}...", mod_info.mod_id, mod_info.download_url);

                        let response = perform_stam_request(
                            &mod_info.download_url,
                            &username,
                            &password_hash,
                            game_id,
                            VERSION,
                            host_port,
                            Some(&tmp_dir),
                            None, // No progress callback for initial mod download
                        ).await;

                        if response.status != 200 {
                            error!("FATAL: Failed to download mod '{}': server returned status {}",
                                mod_info.mod_id, response.status);
                            return Err(format!(
                                "Failed to download mod '{}': HTTP {}",
                                mod_info.mod_id, response.status
                            ).into());
                        }

                        // Get the temp file path (file was already saved by perform_stam_request)
                        let archive_path = std::path::PathBuf::from(response.temp_file_path.ok_or_else(|| {
                            format!("Server returned empty content for mod '{}'", mod_info.mod_id)
                        })?);

                        let archive_filename = response.file_name.unwrap_or_else(|| format!("{}.tar.gz", mod_info.mod_id));

                        // Get file size for logging
                        let file_size = std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0);
                        debug!("  Saved {} ({} bytes)", archive_filename, file_size);

                        // Extract tar.gz archive to mods directory
                        let mod_target_dir = mods_dir.join(&mod_info.mod_id);
                        debug!("  Extracting to {}...", mod_target_dir.display());

                        extract_mod_archive(&archive_path, &mod_target_dir)
                            .map_err(|e| format!("Failed to extract mod '{}': {}", mod_info.mod_id, e))?;

                        debug!("  ✓ Mod '{}' installed successfully", mod_info.mod_id);

                        // Clean up archive file
                        std::fs::remove_file(&archive_path)?;

                        // Immediately load the manifest of the newly downloaded mod
                        // so that its dependencies can be discovered in the next iteration
                        let client_dir = mod_target_dir.join("client");
                        let client_manifest_path = client_dir.join("manifest.json");
                        let root_manifest_path = mod_target_dir.join("manifest.json");

                        let (actual_mod_dir, manifest_path) = if client_manifest_path.exists() {
                            (client_dir, client_manifest_path)
                        } else if root_manifest_path.exists() {
                            (mod_target_dir.clone(), root_manifest_path)
                        } else {
                            warn!("Downloaded mod '{}' has no manifest.json", mod_info.mod_id);
                            continue;
                        };

                        match ModManifest::from_json_file(manifest_path.to_str().unwrap()) {
                            Ok(manifest) => {
                                debug!("  Loaded manifest: {} v{} (dependencies: {:?})",
                                    manifest.name, manifest.version,
                                    manifest.requires.keys().filter(|k| !k.starts_with('@')).collect::<Vec<_>>());
                                available_manifests.insert(mod_info.mod_id.clone(), (manifest, actual_mod_dir));
                                // Remove from missing_mods if it was there
                                missing_mods.retain(|id| id != &mod_info.mod_id);
                            }
                            Err(e) => {
                                warn!("Failed to load manifest for downloaded mod '{}': {}", mod_info.mod_id, e);
                            }
                        }
                    }

                    // Continue loop to check if newly downloaded mods have more dependencies
                }

                // Final check: ensure all bootstrap mods are available
                let still_missing: Vec<&str> = required_bootstrap_mods.iter()
                    .filter(|m| !available_manifests.contains_key(&m.mod_id))
                    .map(|m| m.mod_id.as_str())
                    .collect();

                if !still_missing.is_empty() {
                    error!("FATAL: Bootstrap mod(s) still missing after download: {:?}", still_missing);
                    return Err(format!(
                        "Failed to install bootstrap mod(s): {:?}",
                        still_missing
                    ).into());
                }
            }

            // Initialize mod runtime manager and load ONLY bootstrap mods + their dependencies
            if !available_manifests.is_empty() {
                debug!("Initializing mod runtime system...");

                // Create mod runtime manager
                let mut runtime_manager = ModRuntimeManager::new();

                // Initialize JavaScript runtime (one shared runtime for all JS mods)
                let runtime_config = create_js_runtime_config(&app_paths, &game_id)?;
                let mut js_adapter = JsRuntimeAdapter::new(runtime_config)?;

                // Set home directory for mod installation (used by system.install_mod_from_path)
                js_adapter.system_api().set_home_dir(game_root.clone());

                // Set game info for system.get_game_info() (client-only API)
                js_adapter.system_api().set_game_info(game_id, &game_name, &game_version);

                // Setup graphic proxy for graphic engine operations (client-only)
                // Pass game_root as asset_root so Bevy can load assets from mods directory
                let graphic_proxy = Arc::new(GraphicProxy::new_client(engine_request_tx.clone(), Some(game_root.clone())));
                js_adapter.set_graphic_proxy(graphic_proxy.clone());
                // Save for main loop to poll graphic events
                graphic_proxy_opt = Some(graphic_proxy.clone());

                // Setup locale API for internationalization in JavaScript mods
                // LocaleApi now supports hierarchical lookup: mod locale -> global locale
                // We wrap Arc<LocaleManager> in a Mutex to make it Send+Sync for use in closures
                let locale_mutex: Arc<std::sync::Mutex<Arc<LocaleManager>>> = Arc::new(std::sync::Mutex::new(locale.clone()));
                let locale_for_get = locale_mutex.clone();
                let locale_for_get_args = locale_mutex.clone();
                let locale_api = LocaleApi::new(
                    locale.current_locale(),  // current locale (e.g., "it-IT")
                    "en-US",                  // fallback locale
                    move |id| {
                        let guard = locale_for_get.lock().unwrap();
                        guard.get(id)
                    },
                    move |id, args| {
                        let guard = locale_for_get_args.lock().unwrap();
                        // Convert HashMap<String, String> to FluentArgs
                        let mut fluent_args = fluent_bundle::FluentArgs::new();
                        for (key, value) in args {
                            fluent_args.set(key.as_str(), fluent_bundle::FluentValue::from(value.clone()));
                        }
                        guard.get_with_args(id, Some(&fluent_args))
                    },
                );
                js_adapter.set_locale_api(locale_api);

                // Setup network API for downloading resources via stam:// protocol
                // Capture credentials, game_id, and server address for use in the download callback
                let network_username = username.to_string();
                let network_password_hash = password_hash.clone();
                let network_game_id = game_id.to_string();
                let network_server = host_port.to_string();  // Default server for URIs without host
                let network_config = NetworkConfig {
                    game_id: game_id.to_string(),
                    username: username.to_string(),
                    password_hash: password_hash.clone(),
                    client_version: VERSION.to_string(),
                };
                let mut network_api = NetworkApi::new(network_config);

                // Set the download callback that performs stam:// requests
                // Note: We pass None for tmp_dir because the JS runtime's TempFileManager
                // handles temp file creation after this callback returns
                network_api.set_download_callback(Arc::new(move |uri: String, progress_callback| {
                    let username = network_username.clone();
                    let password_hash = network_password_hash.clone();
                    let game_id = network_game_id.clone();
                    let client_version = VERSION.to_string();
                    let default_server = network_server.clone();

                    Box::pin(async move {
                        perform_stam_request(&uri, &username, &password_hash, &game_id, &client_version, &default_server, None, progress_callback).await
                    })
                }));
                js_adapter.set_network_api(network_api);

                // Get runtime handle BEFORE moving the adapter to the manager
                let js_runtime = js_adapter.get_runtime();

                // Build mod info map for easier lookup (only for available mods)
                struct ModData {
                    mod_id: String,
                    manifest: ModManifest,
                    /// Entry point path - None for asset-only mods (no executable code)
                    entry_point_path: Option<std::path::PathBuf>,
                    /// Absolute entry point - None for asset-only mods
                    absolute_entry_point: Option<std::path::PathBuf>,
                }

                let mut mod_data_map: HashMap<String, ModData> = HashMap::new();

                // First pass: Register mod aliases and collect mod data for AVAILABLE mods only
                // This must happen BEFORE loading any mod, so that import "@mod-id" works
                // Mods without entry_point are asset-only and automatically considered attached
                debug!("Registering mod aliases for available mods...");

                for (mod_id, (manifest, actual_mod_dir)) in &available_manifests {
                    // Check if mod has an entry_point (asset-only mods don't)
                    if let Some(ref entry_point) = manifest.entry_point {
                        // Use actual_mod_dir (could be root or client/ subdirectory)
                        let entry_point_path = actual_mod_dir.join(entry_point);

                        // Convert to absolute path for reliable module resolution
                        let absolute_entry_point = if entry_point_path.is_absolute() {
                            entry_point_path.clone()
                        } else {
                            std::env::current_dir()?.join(&entry_point_path)
                        };

                        // Register alias before loading
                        stam_mod_runtimes::adapters::js::register_mod_alias(
                            mod_id,
                            absolute_entry_point.clone(),
                        );

                        mod_data_map.insert(mod_id.clone(), ModData {
                            mod_id: mod_id.clone(),
                            manifest: manifest.clone(),
                            entry_point_path: Some(entry_point_path),
                            absolute_entry_point: Some(absolute_entry_point),
                        });
                    } else {
                        // Asset-only mod (no entry_point) - no alias to register, auto-attached
                        debug!("Mod '{}' has no entry_point, registering as asset-only (auto-attached)", mod_id);
                        mod_data_map.insert(mod_id.clone(), ModData {
                            mod_id: mod_id.clone(),
                            manifest: manifest.clone(),
                            entry_point_path: None,
                            absolute_entry_point: None,
                        });
                    }
                }

                // Register ALL mods in SystemApi (including missing ones)
                // Available mods get their info from manifest, missing ones get minimal info
                // All mods get download_url from server (for re-download if needed)
                // exists=true for mods found locally, exists=false for missing mods
                // Asset-only mods (no entry_point) are automatically loaded=true
                for mod_info in &mods {
                    if let Some(mod_data) = mod_data_map.get(&mod_info.mod_id) {
                        // Available mod - use manifest info, exists=true
                        // Include archive info from server for display/validation
                        // Asset-only mods (no entry_point) are auto-attached (loaded=true)
                        let is_asset_only = mod_data.entry_point_path.is_none();
                        js_adapter.register_mod_info(ModInfo {
                            id: mod_info.mod_id.clone(),
                            version: mod_data.manifest.version.clone(),
                            name: mod_data.manifest.name.clone(),
                            description: mod_data.manifest.description.clone(),
                            mod_type: mod_data.manifest.mod_type.clone(),
                            priority: mod_data.manifest.priority,
                            bootstrapped: false,
                            loaded: false,  // Asset-only mods are auto-attached
                            exists: true,   // Available locally
                            download_url: Some(mod_info.download_url.clone()),  // Keep URL for potential re-download
                            archive_sha512: if mod_info.archive_sha512.is_empty() { None } else { Some(mod_info.archive_sha512.clone()) },
                            archive_bytes: if mod_info.archive_bytes > 0 { Some(mod_info.archive_bytes) } else { None },
                            uncompressed_bytes: if mod_info.uncompressed_bytes > 0 { Some(mod_info.uncompressed_bytes) } else { None },
                        });
                    } else {
                        // Missing mod - use info from server with placeholder values, exists=false
                        // Include archive info from server for download UI
                        js_adapter.register_mod_info(ModInfo {
                            id: mod_info.mod_id.clone(),
                            version: "?".to_string(),
                            name: mod_info.mod_id.clone(),
                            description: "Not available locally".to_string(),
                            mod_type: Some(mod_info.mod_type.clone()),
                            priority: 999,  // Low priority for missing mods
                            bootstrapped: false,
                            loaded: false,  // Will remain false as it's missing
                            exists: false,  // Not available locally - needs download
                            download_url: Some(mod_info.download_url.clone()),  // Needs download
                            archive_sha512: if mod_info.archive_sha512.is_empty() { None } else { Some(mod_info.archive_sha512.clone()) },
                            archive_bytes: if mod_info.archive_bytes > 0 { Some(mod_info.archive_bytes) } else { None },
                            uncompressed_bytes: if mod_info.uncompressed_bytes > 0 { Some(mod_info.uncompressed_bytes) } else { None },
                        });
                    }
                }

                // Store reference to system API for setting bootstrapped/loaded state later
                let system_api = js_adapter.system_api().clone();

                // Now register the adapter with the runtime manager
                runtime_manager.register_adapter(
                    stam_mod_runtimes::RuntimeType::JavaScript,
                    Box::new(js_adapter),
                );

                // Collect bootstrap mods (only from available mods)
                let bootstrap_mod_ids: Vec<String> = mod_data_map
                    .values()
                    .filter(|md| md.manifest.mod_type.as_deref() == Some("bootstrap"))
                    .map(|md| md.mod_id.clone())
                    .collect();

                // Recursive function to collect dependencies
                fn collect_dependencies(
                    mod_id: &str,
                    mod_data_map: &HashMap<String, ModData>,
                    to_load: &mut Vec<String>,
                    loading_chain: &mut Vec<String>,
                ) -> Result<(), String> {
                    // Loop detection
                    if loading_chain.contains(&mod_id.to_string()) {
                        return Err(format!(
                            "Circular dependency detected: {} -> {}",
                            loading_chain.join(" -> "),
                            mod_id
                        ));
                    }

                    // Already scheduled to load
                    if to_load.contains(&mod_id.to_string()) {
                        return Ok(());
                    }

                    let mod_data = mod_data_map.get(mod_id).ok_or_else(|| {
                        format!("Dependency '{}' not found in available mods", mod_id)
                    })?;

                    // Add to loading chain for loop detection
                    loading_chain.push(mod_id.to_string());

                    // First load all dependencies
                    for (dep_id, _version_req) in &mod_data.manifest.requires {
                        // Skip special requirements like @client, @server, @game
                        if dep_id.starts_with('@') {
                            continue;
                        }
                        // Recursively collect dependencies
                        collect_dependencies(dep_id, mod_data_map, to_load, loading_chain)?;
                    }

                    // Remove from loading chain
                    loading_chain.pop();

                    // Add this mod to load list
                    to_load.push(mod_id.to_string());

                    Ok(())
                }

                // Collect all mods to load (bootstrap + their dependencies, sorted by dependency order)
                let mut mods_to_load: Vec<String> = Vec::new();
                let mut loading_chain: Vec<String> = Vec::new();

                for bootstrap_mod_id in &bootstrap_mod_ids {
                    collect_dependencies(
                        bootstrap_mod_id,
                        &mod_data_map,
                        &mut mods_to_load,
                        &mut loading_chain,
                    )?;
                }

                // Sort mods_to_load by priority (lower priority loads first)
                mods_to_load.sort_by_key(|mod_id| {
                    mod_data_map.get(mod_id).map(|md| md.manifest.priority).unwrap_or(0)
                });

                // Determine which mods are NOT loaded (for mods_notyetloaded list)
                // This includes both available mods not in the load list AND missing mods
                let mut mods_not_loaded: Vec<String> = mod_data_map
                    .keys()
                    .filter(|mod_id| !mods_to_load.contains(mod_id))
                    .cloned()
                    .collect();
                // Add missing mods to the not loaded list
                mods_not_loaded.extend(missing_mods.clone());

                debug!("Mods to load (bootstrap + dependencies): {:?}", mods_to_load);
                if !mods_not_loaded.is_empty() {
                    debug!("Mods deferred for later loading: {:?}", mods_not_loaded);
                }
                if !missing_mods.is_empty() {
                    debug!("  (including {} missing locally: {:?})", missing_mods.len(), missing_mods);
                }

                // Load ONLY bootstrap mods + their dependencies
                // Non-bootstrap mods will be loaded by mods-manager when needed
                // mods_to_load already contains bootstrap + dependencies in correct order
                // Skip asset-only mods (no entry_point) - they are already auto-attached
                debug!("Attaching {} mods (bootstrap + dependencies)...", mods_to_load.len());
                for mod_id in &mods_to_load {
                    let mod_data = mod_data_map.get(mod_id).unwrap();
                    // Skip asset-only mods - they have no code to load
                    if let Some(ref entry_point_path) = mod_data.entry_point_path {
                        runtime_manager.load_mod(mod_id, entry_point_path)?;
                        runtime_manager.call_mod_function(mod_id, "onAttach")?;
                        // Mark mod as loaded in SystemApi
                        system_api.set_loaded(mod_id, true);
                    }
                    // Asset-only mods are already marked as loaded=true during registration
                }

                // Call onBootstrap ONLY for bootstrap mods (not for dependencies)
                // Note: Missing bootstrap mods check is done earlier, before runtime initialization
                if !bootstrap_mod_ids.is_empty() {
                    debug!("Bootstrapping {} mod(s)...", bootstrap_mod_ids.len());
                    for mod_id in &bootstrap_mod_ids {
                        runtime_manager.call_mod_function(mod_id, "onBootstrap")?;
                        // Mark mod as bootstrapped
                        system_api.set_bootstrapped(mod_id, true);
                    }
                }

                // Count deferred mods (available but not loaded yet)
                let deferred_count = mod_data_map.len() - mods_to_load.len();
                debug!("Mod system initialized successfully ({} loaded, {} deferred, {} missing)",
                    mods_to_load.len(), deferred_count, missing_mods.len());
                js_runtime_handle = Some(js_runtime);

                // Save for dynamic mod loading in main loop
                runtime_manager_opt = Some(runtime_manager);
                system_api_opt = Some(system_api);
                game_root_opt = Some(game_root);
            }
        }
        Ok(GameMessage::Error { message }) => {
            // Message from server could be a locale ID
            let localized_msg = locale.get(&message);
            error!(
                "{}",
                locale.get_with_args(
                    "server-error",
                    Some(&fluent_args! {
                        "message" => localized_msg.as_str()
                    })
                )
            );
            return Err(localized_msg.into());
        }
        Ok(msg) => {
            error!("{}: {:?}", locale.get("error-unexpected-message"), msg);
            return Err(locale.get("error-unexpected-message").into());
        }
        Err(e) => {
            error!("{}: {}", locale.get("error-parse-failed"), e);
            return Err(e.into());
        }
    }

    // Maintain connection - wait for messages or Ctrl+C
    // Show appropriate message based on whether any mod registered for TerminalKeyPressed
    let has_terminal_handlers = runtime_manager_opt
        .as_ref()
        .map(|rm| rm.terminal_key_handler_count() > 0)
        .unwrap_or(false);
    if has_terminal_handlers {
        info!("{}", locale.get("game-client-ready-no-hint"));
    } else {
        info!("{}", locale.get("game-client-ready"));
    }

    // Setup SIGTERM handler (Linux/Unix only)
    // This allows graceful shutdown when the process receives SIGTERM
    let sigterm_received = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        let sigterm_flag = sigterm_received.clone();
        tokio::spawn(async move {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut stream) => {
                    stream.recv().await;
                    info!("Received SIGTERM signal");
                    sigterm_flag.store(true, Ordering::Relaxed);
                }
                Err(err) => {
                    warn!("Error setting up SIGTERM handler: {}", err);
                }
            }
        });
    }

    // Run the JS event loop if we have JS mods loaded
    // This is necessary for setTimeout/setInterval to work properly
    if let Some(js_runtime) = js_runtime_handle {
        debug!("Starting JavaScript event loop for timer support");

        // Take the attach request receiver from SystemApi
        let mut attach_rx = if let Some(ref system_api) = system_api_opt {
            system_api.take_attach_receiver().await
        } else {
            None
        };

        // Take the send_event request receiver from EventDispatcher
        let mut send_event_rx = if let Some(ref system_api) = system_api_opt {
            system_api.event_dispatcher().take_send_event_receiver().await
        } else {
            None
        };

        // Take the shutdown request receiver from SystemApi
        let mut shutdown_rx = if let Some(ref system_api) = system_api_opt {
            system_api.take_shutdown_receiver().await
        } else {
            None
        };

        // Take the graphic event receiver from GraphicProxy (if engine is enabled)
        // This receiver gets graphic engine events like EngineReady, WindowCreated, KeyPressed, etc.
        let mut graphic_event_rx = if let Some(ref graphic_proxy) = graphic_proxy_opt {
            graphic_proxy.take_event_receiver().await
        } else {
            None
        };

        // Start terminal input reader if running in a terminal
        let terminal_input_enabled = stam_mod_runtimes::terminal_input::is_terminal();
        let (mut terminal_rx, mut terminal_handle) = if terminal_input_enabled {
            match stam_mod_runtimes::terminal_input::spawn_terminal_event_reader() {
                Ok((rx, handle)) => (Some(rx), Some(handle)),
                Err(e) => {
                    debug!("Failed to start terminal input reader: {}", e);
                    (None, None)
                }
            }
        } else {
            debug!("Not running in terminal, terminal input disabled");
            (None, None)
        };
        let terminal_input_active = terminal_rx.is_some();

        // Pin the JS event loop future to avoid recreating it on each loop iteration
        // This matches the server's optimized pattern for persistent async futures
        let mut js_loop = std::pin::pin!(run_js_event_loop(js_runtime.clone()));

        // Main event loop - handles JS events, attach requests, send_event, shutdown, terminal input, and connection
        loop {
            tokio::select! {
                biased;

                // Handle shutdown requests from JavaScript (system.exit)
                request = async {
                    if let Some(ref mut rx) = shutdown_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Some(request) = request {
                        info!("Shutdown requested by mod with exit code {}", request.exit_code);
                        break;
                    }
                }

                // Handle terminal key events (raw mode input)
                key_request = async {
                    if let Some(ref mut rx) = terminal_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Some(key_request) = key_request {
                        trace!("Terminal key received: key='{}' ctrl={} alt={} shift={}",
                               key_request.key, key_request.ctrl, key_request.alt, key_request.shift);

                        // Dispatch to mods
                        let mut handled = false;
                        if let Some(ref runtime_manager) = runtime_manager_opt {
                            let response = runtime_manager.dispatch_terminal_key(&key_request);
                            handled = response.handled;
                            trace!("Terminal key dispatch returned handled={}", handled);
                        }

                        // Check for Ctrl+C - default exit behavior
                        if !handled && key_request.ctrl && key_request.key == "c" {
                            info!("{}", locale.get("ctrl-c-received"));
                            break;
                        }

                        // If not handled, the key press is "swallowed" (not echoed)
                        // This is the expected behavior in raw mode
                    }
                }

                // Fallback Ctrl+C handler when terminal input is not available
                _ = async {
                    if !terminal_input_active {
                        tokio::signal::ctrl_c().await.ok();
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    info!("{}", locale.get("ctrl-c-received"));
                    break;
                }

                // Handle attach mod requests from JavaScript
                request = async {
                    if let Some(ref mut rx) = attach_rx {
                        rx.recv().await
                    } else {
                        // No receiver, wait forever
                        std::future::pending().await
                    }
                } => {
                    if let Some(request) = request {
                        let result = handle_attach_mod_request(
                            &request.mod_id,
                            &mut runtime_manager_opt,
                            &system_api_opt,
                            &game_root_opt,
                        ).await;
                        // Send response back to JS
                        let _ = request.response_tx.send(result);
                    }
                }

                // Handle send_event requests from JavaScript
                // Both sendEvent (fire-and-forget) and sendEventAsync use this path
                request = async {
                    if let Some(ref mut rx) = send_event_rx {
                        rx.recv().await
                    } else {
                        // No receiver, wait forever
                        std::future::pending().await
                    }
                } => {
                    if let Some(request) = request {
                        let response = handle_send_event_request(
                            &request.event_name,
                            &request.args,
                            &mut runtime_manager_opt,
                        );
                        // Send response back to JS
                        let _ = request.response_tx.send(response);
                    }
                }

                // Handle graphic engine events
                event = async {
                    if let Some(ref mut rx) = graphic_event_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Some(event) = event {
                        handle_graphic_event(
                            event,
                            &mut runtime_manager_opt,
                        );
                    }
                }

                // Maintain game connection
                _ = maintain_game_connection(&mut stream, locale.clone()) => {
                    info!("{}", locale.get("connection-closed"));
                    break;
                }

                // Run JS event loop for timer callbacks (uses pinned future reference)
                fatal_error = &mut js_loop => {
                    if fatal_error {
                        error!("{}", locale.get("js-fatal-error"));
                    } else {
                        debug!("JavaScript event loop exited unexpectedly");
                    }
                    break;
                }

                // Check for SIGTERM and graphic event receiver (polled periodically)
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    if sigterm_received.load(Ordering::Relaxed) {
                        break;
                    }

                    // If we don't have a graphic event receiver yet, try to get one
                    // This handles the case where enableEngine() was called after our initial check
                    if graphic_event_rx.is_none() {
                        if let Some(ref graphic_proxy) = graphic_proxy_opt {
                            if let Some(rx) = graphic_proxy.take_event_receiver().await {
                                debug!("Obtained graphic event receiver after engine enablement");
                                graphic_event_rx = Some(rx);
                            }
                        }
                    }
                }
            }
        }

        // Stop terminal input reader and wait for cleanup to complete
        if let Some(ref mut handle) = terminal_handle {
            handle.stop_async().await;
        }
    } else {
        // No JS runtime, just wait for connection or Ctrl+C
        // Still dispatch TerminalKeyPressed to allow other runtimes to handle it

        // Take the shutdown request receiver from SystemApi (if available)
        let mut shutdown_rx = if let Some(ref system_api) = system_api_opt {
            system_api.take_shutdown_receiver().await
        } else {
            None
        };

        // Start terminal input reader if running in a terminal
        let (mut terminal_rx, mut terminal_handle) = if stam_mod_runtimes::terminal_input::is_terminal() {
            match stam_mod_runtimes::terminal_input::spawn_terminal_event_reader() {
                Ok((rx, handle)) => (Some(rx), Some(handle)),
                Err(e) => {
                    debug!("Failed to start terminal input reader: {}", e);
                    (None, None)
                }
            }
        } else {
            debug!("Not running in terminal, terminal input disabled");
            (None, None)
        };
        let terminal_input_active = terminal_rx.is_some();

        loop {
            tokio::select! {
                biased;

                // Handle shutdown requests from mods (system.exit)
                request = async {
                    if let Some(ref mut rx) = shutdown_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Some(request) = request {
                        info!("Shutdown requested by mod with exit code {}", request.exit_code);
                        break;
                    }
                }

                // Handle terminal key events (raw mode input)
                key_request = async {
                    if let Some(ref mut rx) = terminal_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Some(key_request) = key_request {
                        // Dispatch to mods
                        let mut handled = false;
                        if let Some(ref runtime_manager) = runtime_manager_opt {
                            let response = runtime_manager.dispatch_terminal_key(&key_request);
                            handled = response.handled;
                        }

                        // Check for Ctrl+C - default exit behavior
                        if !handled && key_request.ctrl && key_request.key == "c" {
                            info!("{}", locale.get("ctrl-c-received"));
                            break;
                        }
                    }
                }

                // Fallback Ctrl+C handler when terminal input is not available
                _ = async {
                    if !terminal_input_active {
                        tokio::signal::ctrl_c().await.ok();
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    info!("{}", locale.get("ctrl-c-received"));
                    break;
                }

                // Maintain game connection
                _ = maintain_game_connection(&mut stream, locale.clone()) => {
                    info!("{}", locale.get("connection-closed"));
                    break;
                }

                // Check for SIGTERM (polled periodically)
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    if sigterm_received.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }

        // Stop terminal input reader and wait for cleanup to complete
        if let Some(ref mut handle) = terminal_handle {
            handle.stop_async().await;
        }
    }

    // Shutdown graphic engine if one was enabled
    // This sends a Shutdown command to the engine thread, causing it to exit its main loop
    // and allowing the main thread to proceed with termination
    if let Some(ref graphic_proxy) = graphic_proxy_opt {
        debug!("Shutting down graphic engine...");
        match graphic_proxy.shutdown(std::time::Duration::from_secs(5)).await {
            Ok(()) => debug!("Graphic engine shut down successfully"),
            Err(e) => warn!("Graphic engine shutdown error: {}", e),
        }
    }

    info!("{}", locale.get("game-shutdown"));
    Ok(())
}

/// Handle a request to attach (load and initialize) a mod at runtime
///
/// This is called when JavaScript code calls `system.attach_mod(mod_id)`.
/// It reads the mod's manifest, loads the mod into the runtime, and calls onAttach.
async fn handle_attach_mod_request(
    mod_id: &str,
    runtime_manager_opt: &mut Option<ModRuntimeManager>,
    system_api_opt: &Option<stam_mod_runtimes::api::SystemApi>,
    game_root_opt: &Option<std::path::PathBuf>,
) -> Result<(), String> {
    debug!("Attaching mod '{}' at runtime...", mod_id);

    let runtime_manager = runtime_manager_opt.as_mut()
        .ok_or_else(|| "Runtime manager not available".to_string())?;

    let system_api = system_api_opt.as_ref()
        .ok_or_else(|| "System API not available".to_string())?;

    let game_root = game_root_opt.as_ref()
        .ok_or_else(|| "Game root not available".to_string())?;

    // Find the mod directory (check client/ subdirectory first, then root)
    let mods_dir = game_root.join("mods");
    let mod_dir = mods_dir.join(mod_id);

    if !mod_dir.exists() {
        return Err(format!("Mod directory '{}' not found", mod_dir.display()));
    }

    // Check for client/ subdirectory first
    let client_dir = mod_dir.join("client");
    let actual_mod_dir = if client_dir.exists() && client_dir.join("manifest.json").exists() {
        client_dir
    } else {
        mod_dir.clone()
    };

    // Read manifest
    let manifest_path = actual_mod_dir.join("manifest.json");
    let manifest_content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Failed to read manifest: {}", e))?;

    let manifest: stam_schema::ModManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| format!("Failed to parse manifest: {}", e))?;

    // Check if mod has an entry_point - asset-only mods are auto-attached (skip loading)
    if let Some(ref entry_point) = manifest.entry_point {
        // Build entry point path
        let entry_point_path = actual_mod_dir.join(entry_point);

        // Convert to absolute path for reliable module resolution
        let absolute_entry_point = if entry_point_path.is_absolute() {
            entry_point_path.clone()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Failed to get current dir: {}", e))?
                .join(&entry_point_path)
        };

        // Register mod alias before loading
        stam_mod_runtimes::adapters::js::register_mod_alias(mod_id, absolute_entry_point.clone());

        // Load the mod
        runtime_manager.load_mod(mod_id, &entry_point_path)
            .map_err(|e| format!("Failed to load mod: {}", e))?;

        // Call onAttach
        runtime_manager.call_mod_function(mod_id, "onAttach")
            .map_err(|e| format!("Failed to call onAttach: {}", e))?;

        // Mark mod as loaded in SystemApi
        system_api.set_loaded(mod_id, true);

        debug!("Mod '{}' attached successfully", mod_id);
    } else {
        // Asset-only mod (no entry_point) - already considered attached
        debug!("Mod '{}' is asset-only (no entry_point), skipping load", mod_id);
        // Still mark as loaded in case it wasn't already
        system_api.set_loaded(mod_id, true);
    }

    Ok(())
}

/// Handle a request to dispatch a custom event to all registered handlers
///
/// This is called when JavaScript code calls `system.sendEvent()`.
/// It dispatches the event to all registered handlers using the request/response pattern.
///
/// **IMPORTANT**: Handler response values must be set SYNCHRONOUSLY before any
/// `await` points. Values set after an `await` will not be captured.
///
/// # Arguments
/// * `event_name` - The event name to dispatch
/// * `args` - JSON-serialized arguments to pass to handlers
/// * `runtime_manager_opt` - The runtime manager (if available)
///
/// # Returns
/// A `CustomEventResponse` containing:
/// - `handled: bool` - Whether any handler marked the event as handled
/// - `properties: HashMap` - Custom properties set by handlers
fn handle_send_event_request(
    event_name: &str,
    args: &[String],
    runtime_manager_opt: &mut Option<ModRuntimeManager>,
) -> stam_mod_runtimes::api::CustomEventResponse {
    trace!("Dispatching custom event '{}' with {} args", event_name, args.len());

    let runtime_manager = match runtime_manager_opt.as_mut() {
        Some(rm) => rm,
        None => {
            warn!("Runtime manager not available for custom event '{}'", event_name);
            return stam_mod_runtimes::api::CustomEventResponse::default();
        }
    };

    // Create the request object
    let request = stam_mod_runtimes::api::CustomEventRequest::new(event_name, args.to_vec());

    // Dispatch to all handlers and get the aggregated response
    let response = runtime_manager.dispatch_custom_event(&request);

    trace!("Custom event '{}' dispatched (handled={}, properties={})",
        event_name, response.handled, response.properties.len());

    response
}

/// Handle a graphic engine event
///
/// This is called when the worker thread receives an event from the graphic engine.
/// It dispatches the event to the appropriate handlers in the mod runtime.
fn handle_graphic_event(
    event: GraphicEvent,
    runtime_manager_opt: &mut Option<ModRuntimeManager>,
) {
    match event {
        GraphicEvent::EngineReady => {
            debug!("Graphic engine is ready, dispatching GraphicEngineReady event");

            // Dispatch GraphicEngineReady to all registered handlers
            if let Some(runtime_manager) = runtime_manager_opt.as_ref() {
                let request = GraphicEngineReadyRequest::new();
                let response = runtime_manager.dispatch_graphic_engine_ready(&request);

                if response.handled {
                    debug!("GraphicEngineReady was handled by a mod");
                } else {
                    debug!("GraphicEngineReady was not handled (no handlers or all handlers declined)");
                }
            } else {
                debug!("No runtime manager available to dispatch GraphicEngineReady");
            }
        }
        GraphicEvent::WindowCreated { window_id } => {
            trace!("Window {} created", window_id);
            warn!("TODO: Dispatch window:created event to mods");
        }
        GraphicEvent::WindowClosed { window_id } => {
            trace!("Window {} closed, dispatching GraphicEngineWindowClosed event", window_id);

            // Dispatch GraphicEngineWindowClosed to all registered handlers
            if let Some(runtime_manager) = runtime_manager_opt.as_ref() {
                let request = GraphicEngineWindowClosedRequest::new(window_id);
                let response = runtime_manager.dispatch_graphic_engine_window_closed(&request);

            //     if response.handled {
            //         debug!("GraphicEngineWindowClosed was handled by a mod");
            //     } else {
            //         debug!("GraphicEngineWindowClosed was not handled (no handlers or all handlers declined)");
            //     }
            // } else {
            //     debug!("No runtime manager available to dispatch GraphicEngineWindowClosed");
            }
        }
        GraphicEvent::WindowResized { window_id, width, height } => {
            trace!("Window {} resized to {}x{}", window_id, width, height);
            warn!("TODO: Dispatch window:resized event to mods");
        }
        GraphicEvent::WindowFocused { window_id, focused } => {
            trace!("Window {} focus changed: {}", window_id, focused);
            warn!("TODO: Dispatch window:focused event to mods");
        }
        GraphicEvent::WindowMoved { window_id, x, y } => {
            trace!("Window {} moved to ({}, {})", window_id, x, y);
            warn!("TODO: Dispatch window:moved event to mods");
        }
        GraphicEvent::KeyPressed { window_id, key, modifiers } => {
            trace!("Key pressed in window {}: {} (mods: {:?})", window_id, key, modifiers);
            warn!("TODO: Dispatch input:keyPressed event to mods");
        }
        GraphicEvent::KeyReleased { window_id, key, modifiers } => {
            trace!("Key released in window {}: {} (mods: {:?})", window_id, key, modifiers);
            warn!("TODO: Dispatch input:keyReleased event to mods");
        }
        GraphicEvent::CharacterInput { window_id, character } => {
            trace!("Character input in window {}: '{}'", window_id, character);
            warn!("TODO: Dispatch input:character event to mods");
        }
        GraphicEvent::MouseMoved { window_id, x, y } => {
            // Too verbose for debug, use trace if needed
            // trace!("Mouse moved in window {}: ({}, {})", window_id, x, y);
            let _ = (window_id, x, y); // Suppress unused warnings
            warn!("TODO: Dispatch input:mouseMoved event to mods");
        }
        GraphicEvent::MouseButtonPressed { window_id, button, x, y } => {
            trace!("Mouse button {:?} pressed in window {} at ({}, {})", button, window_id, x, y);
            warn!("TODO: Dispatch input:mousePressed event to mods");
        }
        GraphicEvent::MouseButtonReleased { window_id, button, x, y } => {
            trace!("Mouse button {:?} released in window {} at ({}, {})", button, window_id, x, y);
            warn!("TODO: Dispatch input:mouseReleased event to mods");
        }
        GraphicEvent::MouseWheel { window_id, delta_x, delta_y } => {
            trace!("Mouse wheel in window {}: ({}, {})", window_id, delta_x, delta_y);
            warn!("TODO: Dispatch input:mouseWheel event to mods");
        }
        GraphicEvent::FrameStart { window_id, delta_time } => {
            // Too verbose for debug
            let _ = (window_id, delta_time);
            //warn!("TODO: Dispatch frame:start event to mods if needed");
        }
        GraphicEvent::FrameEnd { window_id, frame_time } => {
            // Too verbose for debug
            let _ = (window_id, frame_time);
            //warn!("TODO: Dispatch frame:end event to mods if needed");
        }
        GraphicEvent::EngineError { message } => {
            error!("Graphic engine error: {}", message);
            warn!("TODO: Dispatch engine:error event to mods");
        }
        GraphicEvent::EngineShuttingDown => {
            info!("Graphic engine is shutting down");
            warn!("TODO: Dispatch engine:shuttingDown event to mods");
        }
        // Widget events
        GraphicEvent::WidgetCreated { window_id, widget_id, widget_type } => {
            trace!("Widget {} ({:?}) created in window {}", widget_id, widget_type, window_id);
            warn!("TODO: Dispatch widget:created event to mods");
        }
        GraphicEvent::WidgetDestroyed { window_id, widget_id } => {
            trace!("Widget {} destroyed in window {}", widget_id, window_id);
            warn!("TODO: Dispatch widget:destroyed event to mods");
        }
        GraphicEvent::WidgetClicked { window_id, widget_id, x, y, button } => {
            trace!("Widget {} clicked in window {} at ({}, {}) with {:?}", widget_id, window_id, x, y, button);

            // Dispatch widget click event to mods
            if let Some(runtime_manager) = runtime_manager_opt.as_ref() {
                // Create event data object
                let event_data = serde_json::json!({
                    "windowId": window_id,
                    "widgetId": widget_id,
                    "x": x,
                    "y": y,
                    "button": button.as_str(),
                });

                if let Err(e) = runtime_manager.dispatch_widget_event(widget_id, "click", event_data) {
                    error!("Failed to dispatch widget click event: {}", e);
                }
            }
        }
        GraphicEvent::WidgetHovered { window_id, widget_id, entered, x, y } => {
            trace!("Widget {} hover {} in window {} at ({}, {})", widget_id, if entered { "enter" } else { "leave" }, window_id, x, y);
            warn!("TODO: Dispatch widget:hovered event to mods");
        }
        GraphicEvent::WidgetFocused { window_id, widget_id, focused } => {
            trace!("Widget {} focus {} in window {}", widget_id, if focused { "gained" } else { "lost" }, window_id);
            warn!("TODO: Dispatch widget:focused event to mods");
        }
        GraphicEvent::WidgetInteractionChanged { window_id, widget_id, interaction } => {
            trace!("Widget {} interaction changed to '{}' in window {}", widget_id, interaction, window_id);
            // Internal event for tracking, usually not dispatched
        }
    }
}

/// Maintain game connection - read messages from server
async fn maintain_game_connection(stream: &mut TcpStream, locale: Arc<LocaleManager>) {
    loop {
        match stream.read_game_message().await {
            Ok(GameMessage::Disconnect { message }) => {
                // Message is a locale ID (e.g., "disconnect-server-shutdown")
                let localized_msg = locale.get(&message);
                info!("{}", localized_msg);
                break;
            }
            Ok(GameMessage::Error { message }) => {
                // Message could be a locale ID
                let localized_msg = locale.get(&message);
                error!(
                    "{}",
                    locale.get_with_args(
                        "server-error",
                        Some(&fluent_args! {
                            "message" => localized_msg.as_str()
                        })
                    )
                );
                break;
            }
            Ok(msg) => {
                debug!("Received game message: {:?}", msg);
                warn!("TODO: Handle other game messages");
            }
            Err(e) => {
                debug!("Connection closed: {}", e);
                break;
            }
        }
    }
}

/// Staminal Client - Connect to Staminal servers
#[derive(Parser, Debug)]
#[command(name = "stam_client")]
#[command(author = "Staminal Project")]
#[command(version = VERSION)]
#[command(about = "Staminal Game Client", long_about = None)]
struct Args {
    /// Server URI (e.g., stam://username:password@host:port or from STAM_URI env)
    #[arg(short, long, env = "STAM_URI")]
    uri: String,

    /// Assets directory path (default: ./assets)
    #[arg(short, long, default_value = "assets")]
    assets: String,

    /// Language/Locale to use (e.g., en-US, it-IT) - overrides system locale
    #[arg(short, long, env = "STAM_LANG")]
    lang: Option<String>,

    /// Custom home directory for Staminal data and config (overrides system directories)
    /// Useful for development and testing
    #[arg(long, env = "STAM_HOME")]
    home: Option<String>,

    /// Enable logging to file (stam_client.log in current directory)
    #[arg(long, env = "STAM_LOG_FILE")]
    log_file: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, env = "STAM_LOG_LEVEL", default_value = "info")]
    log_level: String,
}

// ============================================================================
// Main Entry Point
// ============================================================================

/// Main entry point - runs on the main thread
///
/// The main thread hosts graphic engines (like Bevy) which require the main thread
/// on some platforms (macOS, Windows). All other client logic (networking, mods,
/// JS runtime) runs in a worker thread.
fn main() {
    // Parse args first to check if log file is requested
    let args = Args::parse();

    // Setup logging (must happen on main thread before spawning worker)
    setup_logging(&args);

    info!("========================================");
    info!("   STAMINAL CLIENT v{}", VERSION);
    info!("========================================");

    // Create communication channels between main and worker thread
    let (worker_tx, main_rx) = std_mpsc::channel::<WorkerMessage>();
    let (_main_tx, worker_rx) = std_mpsc::channel::<MainMessage>();

    // Create channel for graphic engine enable requests
    let (engine_request_tx, engine_request_rx) = std_mpsc::channel::<EnableEngineRequest>();

    // Spawn the worker thread that runs all client logic
    let worker_handle = std::thread::Builder::new()
        .name("client-worker".to_string())
        .spawn(move || {
            worker_thread_main(args, worker_tx, worker_rx, engine_request_tx);
        })
        .expect("Failed to spawn worker thread");

    // Main loop - wait for worker thread completion or graphic engine requests
    let exit_code = loop {
        // Check for engine enable requests (non-blocking)
        if let Ok(request) = engine_request_rx.try_recv() {
            match request.engine_type {
                GraphicEngines::Bevy => {
                    info!("Main thread: Starting Bevy graphic engine");

                    // Create event channel for engine -> worker communication
                    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<GraphicEvent>(256);

                    // Create command channel for worker -> engine communication
                    let (cmd_tx, cmd_rx) = std_mpsc::channel::<GraphicCommand>();

                    // Send channels back to worker thread
                    if request.response_tx.send(Ok((cmd_tx, event_rx))).is_err() {
                        error!("Failed to send engine channels to worker thread");
                        break 1;
                    }

                    // Create and run Bevy engine on main thread (blocks until shutdown)
                    // Pass the initial window config and asset_root from the request
                    let mut engine = BevyEngine::new(event_tx);
                    use stam_mod_runtimes::api::GraphicEngine;
                    engine.run(cmd_rx, request.initial_window_config, request.asset_root);

                    info!("Bevy engine has shut down");
                    // Engine has exited, continue to check for worker termination
                }
                other => {
                    warn!(
                        "Unsupported graphic engine requested: {:?}",
                        other
                    );
                    let _ = request.response_tx.send(Err(format!(
                        "Graphic engine '{:?}' is not yet supported",
                        other
                    )));
                }
            }
        }

        // Check for worker termination (non-blocking)
        match main_rx.try_recv() {
            Ok(WorkerMessage::Terminated { exit_code }) => {
                debug!("Worker thread terminated with exit code {}", exit_code);
                break exit_code;
            }
            Ok(WorkerMessage::Error { message }) => {
                error!("Worker thread error: {}", message);
                break 1;
            }
            Err(std_mpsc::TryRecvError::Empty) => {
                // No message yet, continue polling
            }
            Err(std_mpsc::TryRecvError::Disconnected) => {
                // Channel closed unexpectedly - worker thread panicked or crashed
                error!("Worker thread communication channel closed unexpectedly");
                break 1;
            }
        }

        // Small sleep to avoid busy-loop (10ms)
        std::thread::sleep(std::time::Duration::from_millis(10));
    };

    // Wait for the worker thread to fully terminate
    if let Err(e) = worker_handle.join() {
        error!("Worker thread panicked: {:?}", e);
    }

    debug!("Main thread exiting with code {}", exit_code);
    std::process::exit(exit_code);
}

/// Setup logging (called from main thread)
///
/// Uses STAM_LOG_LEVEL environment variable or --log-level argument to set log level.
/// Uses STAM_LOGDEPS environment variable to control dependency logging:
/// - STAM_LOGDEPS=0 (default): Only show logs from Staminal code
/// - STAM_LOGDEPS=1: Show all logs including external dependencies (bevy, wgpu, etc.)
fn setup_logging(args: &Args) {
    let level = match args.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::DEBUG,
    };

    let config = if args.log_file {
        let file = std::fs::File::create("stam_client.log")
            .expect("Unable to create stam_client.log");
        LogConfig::new("stam_client::")
            .with_level(level)
            .with_log_file(file)
    } else {
        LogConfig::<std::fs::File>::new("stam_client::")
            .with_level(level)
    };

    init_logging(config).expect("Failed to initialize logging");
}

// ============================================================================
// Worker Thread
// ============================================================================

/// Worker thread entry point
///
/// Creates a tokio runtime and runs all async client logic.
/// This thread handles networking, mod loading, and the JS event loop.
fn worker_thread_main(
    args: Args,
    worker_tx: std_mpsc::Sender<WorkerMessage>,
    _main_rx: std_mpsc::Receiver<MainMessage>,
    engine_request_tx: std_mpsc::Sender<EnableEngineRequest>,
) {
    // Create a multi-threaded tokio runtime for this worker
    // We need multi-threaded because the JS runtime uses block_on internally
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    // Run the async client logic
    let exit_code = runtime.block_on(async {
        run_client(args, engine_request_tx).await
    });

    // Notify main thread that we're done
    let _ = worker_tx.send(WorkerMessage::Terminated { exit_code });
}

/// Main async client logic (runs in worker thread)
///
/// Returns an exit code (0 = success, non-zero = error)
async fn run_client(args: Args, engine_request_tx: std_mpsc::Sender<EnableEngineRequest>) -> i32 {
    // Check if custom home is specified
    let custom_home = args.home.as_deref();
    if let Some(home) = custom_home {
        debug!("Using custom home directory: {}", home);
    }

    // Initialize application paths (once at startup)
    let app_paths = match AppPaths::new(custom_home) {
        Ok(paths) => paths,
        Err(e) => {
            error!("Failed to initialize application paths: {}", e);
            return 1;
        }
    };

    // Initialize locale manager (wrapped in Arc for sharing with JS runtime)
    let locale = match LocaleManager::new(&args.assets, args.lang.as_deref()) {
        Ok(lm) => Arc::new(lm),
        Err(e) => {
            error!("Failed to initialize locale system: {}", e);
            error!("Continuing without localization support");
            return 1;
        }
    };

    // Parse URI
    if !args.uri.starts_with("stam://") {
        error!(
            "{}",
            locale.get_with_args(
                "error-invalid-uri",
                Some(&fluent_args! {
                    "uri" => args.uri.as_str()
                })
            )
        );
        return 1;
    }

    let uri_without_scheme = args.uri.strip_prefix("stam://").unwrap();

    // Parse username:password@host:port
    let (credentials, host_port) = if let Some(at_pos) = uri_without_scheme.find('@') {
        let creds = &uri_without_scheme[..at_pos];
        let host = &uri_without_scheme[at_pos + 1..];
        (Some(creds), host)
    } else {
        error!(
            "{}",
            locale.get_with_args(
                "error-invalid-uri",
                Some(&fluent_args! {
                    "uri" => args.uri.as_str()
                })
            )
        );
        return 1;
    };

    let (username, password) = if let Some(creds) = credentials {
        if let Some(colon_pos) = creds.find(':') {
            let user = &creds[..colon_pos];
            let pass = &creds[colon_pos + 1..];
            (user.to_string(), pass.to_string())
        } else {
            error!(
                "{}",
                locale.get_with_args(
                    "error-invalid-uri",
                    Some(&fluent_args! {
                        "uri" => args.uri.as_str()
                    })
                )
            );
            return 1;
        }
    } else {
        error!(
            "{}",
            locale.get_with_args(
                "error-invalid-uri",
                Some(&fluent_args! {
                    "uri" => args.uri.as_str()
                })
            )
        );
        return 1;
    };

    info!(
        "{}",
        locale.get_with_args(
            "connecting",
            Some(&fluent_args! {
                "host" => host_port
            })
        )
    );

    // Connect to server
    let mut stream = match TcpStream::connect(host_port).await {
        Ok(s) => {
            info!(
                "{}",
                locale.get_with_args(
                    "connected",
                    Some(&fluent_args! {
                        "host" => host_port
                    })
                )
            );
            s
        }
        Err(e) => {
            error!(
                "{}",
                locale.get_with_args(
                    "connection-failed",
                    Some(&fluent_args! {
                        "error" => e.to_string().as_str()
                    })
                )
            );
            return 1;
        }
    };

    // Read Welcome message
    match stream.read_primal_message().await {
        Ok(PrimalMessage::Welcome { version }) => {
            info!(
                "{}",
                locale.get_with_args(
                    "server-welcome",
                    Some(&fluent_args! {
                        "version" => version.as_str()
                    })
                )
            );

            // Check version compatibility (major.minor must match)
            let client_version_parts: Vec<&str> = VERSION.split('.').collect();
            let server_version_parts: Vec<&str> = version.split('.').collect();

            if client_version_parts.len() >= 2 && server_version_parts.len() >= 2 {
                let client_major_minor =
                    format!("{}.{}", client_version_parts[0], client_version_parts[1]);
                let server_major_minor =
                    format!("{}.{}", server_version_parts[0], server_version_parts[1]);

                if client_major_minor != server_major_minor {
                    error!(
                        "{}",
                        locale.get_with_args(
                            "version-mismatch",
                            Some(&fluent_args! {
                                "client" => VERSION,
                                "server" => version.as_str()
                            })
                        )
                    );
                    return 1;
                }

                info!(
                    "{}",
                    locale.get_with_args(
                        "version-compatible",
                        Some(&fluent_args! {
                            "client" => VERSION,
                            "server" => version.as_str()
                        })
                    )
                );
            }
        }
        Ok(msg) => {
            error!("{}: {:?}", locale.get("error-unexpected-message"), msg);
            return 1;
        }
        Err(e) => {
            error!("{}: {}", locale.get("error-parse-failed"), e);
            return 1;
        }
    }

    // Send Intent with PrimalLogin
    info!("{}", locale.get("login-sending"));

    // Hash password with SHA-512
    let password_hash = sha512_hash(&password);

    let intent = PrimalMessage::Intent {
        intent_type: IntentType::PrimalLogin,
        client_version: VERSION.to_string(),
        username: username.clone(),
        password_hash,
        game_id: None, // Not needed for PrimalLogin
        uri: None,
    };

    if let Err(e) = stream.write_primal_message(&intent).await {
        error!("{}: {}", locale.get("login-failed"), e);
        return 1;
    }

    // Wait for ServerList or Error
    match stream.read_primal_message().await {
        Ok(PrimalMessage::ServerList { servers }) => {
            info!(
                "{}",
                locale.get_with_args(
                    "server-list-received",
                    Some(&fluent_args! {
                        "count" => servers.len()
                    })
                )
            );

            if servers.is_empty() {
                warn!("{}", locale.get("server-list-empty"));
                return 1;
            }

            for (i, server) in servers.iter().enumerate() {
                debug!(
                    "  [{}] {} (game_id: {}) - {}",
                    i + 1,
                    server.name,
                    server.game_id,
                    server.uri
                );
            }

            // Connect to first server in list
            let first_server = &servers[0];
            debug!(
                "Attempting to connect to game server: {} (game_id: {}, uri: {})",
                first_server.name, first_server.game_id, first_server.uri
            );

            // Parse game server URI and connect
            if let Err(e) = connect_to_game_server(
                &first_server.uri,
                &username,
                &password,
                &first_server.game_id,
                locale.clone(),
                &app_paths,
                engine_request_tx,
            )
            .await
            {
                error!(
                    "{}",
                    locale.get_with_args(
                        "connection-failed",
                        Some(&fluent_args! {
                            "error" => e.to_string().as_str()
                        })
                    )
                );
                return 1;
            }
        }
        Ok(PrimalMessage::Error { message }) => {
            // Message could be a locale ID
            let localized_msg = locale.get(&message);
            error!(
                "{}",
                locale.get_with_args(
                    "server-error",
                    Some(&fluent_args! {
                        "message" => localized_msg.as_str()
                    })
                )
            );
            return 1;
        }
        Ok(msg) => {
            error!("{}: {:?}", locale.get("error-unexpected-message"), msg);
            return 1;
        }
        Err(e) => {
            error!("{}: {}", locale.get("error-parse-failed"), e);
            return 1;
        }
    }

    // Success
    0
}
