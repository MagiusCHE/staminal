use clap::Parser;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use tokio::net::TcpStream;
use tracing::{Level, debug, error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use stam_mod_runtimes::logging::{create_custom_timer, CustomFormatter};
use stam_protocol::{GameMessage, GameStream, IntentType, PrimalMessage, PrimalStream};
use stam_schema::{ModManifest, Validatable, validate_mod_dependencies, validate_version_range};

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

/// Connect to game server and maintain connection
async fn connect_to_game_server(
    uri: &str,
    username: &str,
    password: &str,
    game_id: &str,
    locale: &LocaleManager,
    app_paths: &AppPaths,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize game-specific directory
    info!("Initializing directories for game '{}'...", game_id);
    let game_root = app_paths.game_root(game_id)?;
    let mods_dir = game_root.join("mods");

    info!("Game directories:");
    info!("  Root: {}", game_root.display());
    info!("  Mods: {}", mods_dir.display());

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
        password_hash,
        game_id: Some(game_id.to_string()),
    };

    stream.write_primal_message(&intent).await?;

    // Wait for LoginSuccess
    // JS runtime handle for event loop integration
    let mut js_runtime_handle: Option<std::sync::Arc<stam_mod_runtimes::JsAsyncRuntime>> = None;

    match stream.read_game_message().await {
        Ok(GameMessage::LoginSuccess { game_name, game_version, mods }) => {
            info!("{} {} [{}]", locale.get("game-login-success"), game_name, game_version);
            let active_game_version = game_version.clone();

            // Log mod list received
            if !mods.is_empty() {
                info!("Received {} required mod(s):", mods.len());
                // for mod_info in &mods {
                //     info!(
                //         "  - {} [{}]: {}",
                //         mod_info.mod_id, mod_info.mod_type, mod_info.download_url
                //     );
                // }
            } else {
                info!("No mods required for this game");
            }

            // Load manifests only for mods that are present locally
            // Missing mods are tracked separately - we only fail if a required mod is missing
            let mut available_manifests: HashMap<String, ModManifest> = HashMap::new();
            let mut missing_mods: Vec<String> = Vec::new();

            if !mods.is_empty() {
                info!("Server requires {} mod(s), checking local availability...", mods.len());

                // First pass: load manifests for available mods, track missing ones
                for mod_info in &mods {
                    let mod_dir = mods_dir.join(&mod_info.mod_id);

                    // Check if mod directory exists
                    if !mod_dir.exists() {
                        debug!("Mod '{}' not found locally", mod_info.mod_id);
                        missing_mods.push(mod_info.mod_id.clone());
                        continue;
                    }

                    // Read manifest
                    let manifest_path = mod_dir.join("manifest.json");
                    if !manifest_path.exists() {
                        warn!("Mod '{}' directory exists but missing manifest.json", mod_info.mod_id);
                        missing_mods.push(mod_info.mod_id.clone());
                        continue;
                    }

                    let manifest = match ModManifest::from_json_file(manifest_path.to_str().unwrap()) {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("Failed to load manifest for mod '{}': {}", mod_info.mod_id, e);
                            missing_mods.push(mod_info.mod_id.clone());
                            continue;
                        }
                    };

                    info!(" ✓ {} [{}:{}] found", mod_info.mod_id, mod_info.mod_type, manifest.version);
                    available_manifests.insert(mod_info.mod_id.clone(), manifest);
                }

                if !missing_mods.is_empty() {
                    info!(" ? {} mod(s) not available locally: {:?}", missing_mods.len(), missing_mods);
                }
            } else {
                info!("No mods required");
            }

            // Initialize mod runtime manager and load ONLY bootstrap mods + their dependencies
            if !available_manifests.is_empty() {
                info!("Initializing mod runtime system...");

                // Create mod runtime manager
                let mut runtime_manager = ModRuntimeManager::new();

                // Initialize JavaScript runtime (one shared runtime for all JS mods)
                let runtime_config = create_js_runtime_config(&app_paths, &game_id)?;
                let js_adapter = JsRuntimeAdapter::new(runtime_config)?;

                // Get runtime handle BEFORE moving the adapter to the manager
                let js_runtime = js_adapter.get_runtime();

                // Build mod info map for easier lookup (only for available mods)
                struct ModData {
                    mod_id: String,
                    manifest: ModManifest,
                    entry_point_path: std::path::PathBuf,
                    absolute_entry_point: std::path::PathBuf,
                }

                let mut mod_data_map: HashMap<String, ModData> = HashMap::new();

                // First pass: Register mod aliases and collect mod data for AVAILABLE mods only
                // This must happen BEFORE loading any mod, so that import "@mod-id" works
                info!("Registering mod aliases for available mods...");

                for (mod_id, manifest) in &available_manifests {
                    let mod_dir = mods_dir.join(mod_id);
                    let entry_point_path = mod_dir.join(&manifest.entry_point);

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
                        entry_point_path,
                        absolute_entry_point,
                    });
                }

                // Register ALL mods in SystemApi (including missing ones)
                // Available mods get their info from manifest, missing ones get minimal info
                for mod_info in &mods {
                    if let Some(mod_data) = mod_data_map.get(&mod_info.mod_id) {
                        // Available mod - use manifest info
                        js_adapter.register_mod_info(ModInfo {
                            id: mod_info.mod_id.clone(),
                            version: mod_data.manifest.version.clone(),
                            name: mod_data.manifest.name.clone(),
                            description: mod_data.manifest.description.clone(),
                            mod_type: mod_data.manifest.mod_type.clone(),
                            priority: mod_data.manifest.priority,
                            bootstrapped: false,
                            loaded: false,  // Will be set to true when actually loaded
                        });
                    } else {
                        // Missing mod - use info from server with placeholder values
                        js_adapter.register_mod_info(ModInfo {
                            id: mod_info.mod_id.clone(),
                            version: "?".to_string(),
                            name: mod_info.mod_id.clone(),
                            description: "Not available locally".to_string(),
                            mod_type: Some(mod_info.mod_type.clone()),
                            priority: 999,  // Low priority for missing mods
                            bootstrapped: false,
                            loaded: false,  // Will remain false as it's missing
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

                info!("Mods to load (bootstrap + dependencies): {:?}", mods_to_load);
                if !mods_not_loaded.is_empty() {
                    info!("Mods deferred for later loading: {:?}", mods_not_loaded);
                }
                if !missing_mods.is_empty() {
                    info!("  (including {} missing locally: {:?})", missing_mods.len(), missing_mods);
                }

                // Load only the selected mods (bootstrap + dependencies)
                info!("Attaching {} mods...", mods_to_load.len());
                for mod_id in &mods_to_load {
                    let mod_data = mod_data_map.get(mod_id).unwrap();
                    runtime_manager.load_mod(mod_id, &mod_data.entry_point_path)?;
                    runtime_manager.call_mod_function(mod_id, "onAttach")?;
                    // Mark mod as loaded in SystemApi
                    system_api.set_loaded(mod_id, true);
                }

                // Call onBootstrap ONLY for bootstrap mods (not for dependencies)
                if !bootstrap_mod_ids.is_empty() {
                    info!("Bootstrapping {} mod(s)...", bootstrap_mod_ids.len());
                    for mod_id in &bootstrap_mod_ids {
                        runtime_manager.call_mod_function(mod_id, "onBootstrap")?;
                        // Mark mod as bootstrapped
                        system_api.set_bootstrapped(mod_id, true);
                    }
                }

                info!("Mod system initialized successfully ({} loaded, {} deferred)",
                    mods_to_load.len(), mods_not_loaded.len());
                js_runtime_handle = Some(js_runtime);
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
    info!("{}", locale.get("game-client-ready"));

    // Run the JS event loop if we have JS mods loaded
    // This is necessary for setTimeout/setInterval to work properly
    if let Some(js_runtime) = js_runtime_handle {
        debug!("Starting JavaScript event loop for timer support");
        tokio::select! {
            biased;

            // Handle Ctrl+C first
            _ = tokio::signal::ctrl_c() => {
                info!("{}", locale.get("ctrl-c-received"));
            }

            // Maintain game connection
            _ = maintain_game_connection(&mut stream, locale) => {
                info!("{}", locale.get("connection-closed"));
            }

            // Run JS event loop for timer callbacks
            fatal_error = run_js_event_loop(js_runtime) => {
                if fatal_error {
                    error!("{}", locale.get("js-fatal-error"));
                } else {
                    debug!("JavaScript event loop exited unexpectedly");
                }
            }
        }
    } else {
        // No JS runtime, just wait for connection or Ctrl+C
        tokio::select! {
            _ = maintain_game_connection(&mut stream, locale) => {
                info!("{}", locale.get("connection-closed"));
            }
            _ = tokio::signal::ctrl_c() => {
                info!("{}", locale.get("ctrl-c-received"));
            }
        }
    }

    info!("{}", locale.get("game-shutdown"));
    Ok(())
}

/// Maintain game connection - read messages from server
async fn maintain_game_connection(stream: &mut TcpStream, locale: &LocaleManager) {
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
                // TODO: Handle other game messages
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
}

#[tokio::main]
async fn main() {
    // Parse args first to check if log file is requested
    let args = Args::parse();

    // Setup logging
    let timer = create_custom_timer();

    // Disable ANSI colors if:
    // - stdout is not a TTY (piped/redirected)
    // - NO_COLOR env var is set (https://no-color.org/)
    // - TERM=dumb
    let use_ansi = atty::is(atty::Stream::Stdout)
        && std::env::var("NO_COLOR").is_err()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true);

    // Setup logging based on whether file logging is enabled
    if args.log_file {
        // File logging: no ANSI colors
        let file_appender = std::fs::File::create("stam_client.log")
            .expect("Unable to create stam_client.log");
        let formatter_stdout = CustomFormatter::new(timer.clone(), use_ansi)
            .with_strip_prefix("stam_client::");
        let formatter_file = CustomFormatter::new(timer, false)
            .with_strip_prefix("stam_client::");

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_stdout)
                    .with_writer(std::io::stdout),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_file)
                    .with_writer(file_appender),
            )
            .with(tracing_subscriber::filter::LevelFilter::from_level(
                Level::DEBUG,
            ))
            .init();
    } else {
        let formatter = CustomFormatter::new(timer, use_ansi)
            .with_strip_prefix("stam_client::");
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter)
                    .with_writer(std::io::stdout),
            )
            .with(tracing_subscriber::filter::LevelFilter::from_level(
                Level::DEBUG,
            ))
            .init();
    }

    info!("========================================");
    info!("   STAMINAL CLIENT v{}", VERSION);
    info!("========================================");

    // Check if custom home is specified
    let custom_home = args.home.as_deref();
    if let Some(home) = custom_home {
        info!("Using custom home directory: {}", home);
    }

    // Initialize application paths (once at startup)
    let app_paths = match AppPaths::new(custom_home) {
        Ok(paths) => paths,
        Err(e) => {
            error!("Failed to initialize application paths: {}", e);
            return;
        }
    };

    // Initialize locale manager
    let locale = match LocaleManager::new(&args.assets, args.lang.as_deref()) {
        Ok(lm) => lm,
        Err(e) => {
            error!("Failed to initialize locale system: {}", e);
            error!("Continuing without localization support");
            return;
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
        return;
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
        return;
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
            return;
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
        return;
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
            return;
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
                    return;
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
            return;
        }
        Err(e) => {
            error!("{}: {}", locale.get("error-parse-failed"), e);
            return;
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
    };

    if let Err(e) = stream.write_primal_message(&intent).await {
        error!("{}: {}", locale.get("login-failed"), e);
        return;
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
                return;
            }

            for (i, server) in servers.iter().enumerate() {
                info!(
                    "  [{}] {} (game_id: {}) - {}",
                    i + 1,
                    server.name,
                    server.game_id,
                    server.uri
                );
            }

            // Connect to first server in list
            let first_server = &servers[0];
            info!(
                "Attempting to connect to game server: {} (game_id: {}, uri: {})",
                first_server.name, first_server.game_id, first_server.uri
            );

            // Parse game server URI and connect
            if let Err(e) = connect_to_game_server(
                &first_server.uri,
                &username,
                &password,
                &first_server.game_id,
                &locale,
                &app_paths,
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
        }
        Ok(msg) => {
            error!("{}: {:?}", locale.get("error-unexpected-message"), msg);
        }
        Err(e) => {
            error!("{}: {}", locale.get("error-parse-failed"), e);
        }
    }
}
