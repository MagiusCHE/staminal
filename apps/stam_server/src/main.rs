use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::time::{Duration, interval};
use tracing::{Level, debug, error, info, trace, warn};

use stam_mod_runtimes::adapters::js::run_js_event_loop;
use stam_log::{LogConfig, init_logging};
use stam_schema::Validatable;

mod config;
use config::Config;

mod primal_client;
use primal_client::PrimalClient;

mod game_client;

mod client_manager;
use client_manager::ClientManager;

mod mod_loader;

const VERSION: &str = "0.1.0";

async fn wait_for_shutdown(flag: Arc<AtomicBool>) {
    while !flag.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Get default config path based on executable location
fn default_config_path() -> String {
    env::current_exe()
        .ok()
        .and_then(|exe_path| {
            let stem = exe_path.file_stem()?;
            let parent = exe_path.parent()?;
            Some(parent.join(stem).with_extension("json"))
        })
        .and_then(|path| path.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "./stam_server.json".to_string())
}

/// Staminal Core Server - Undifferentiated Game Engine
#[derive(Parser, Debug)]
#[command(name = "stam_server")]
#[command(author = "Staminal Project")]
#[command(version = VERSION)]
#[command(about = "Staminal Game Engine Core Server", long_about = None)]
struct Args {
    /// Path to configuration file (JSON)
    #[arg(short, long, default_value_t = default_config_path())]
    config: String,

    /// Custom home directory for Staminal data and mods (overrides defaults)
    #[arg(long, env = "STAM_HOME")]
    home: Option<String>,

    /// Enable logging to file (stam_server.log in current directory)
    #[arg(long, env = "STAM_LOG_FILE")]
    log_file: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Load configuration first to get log level
    // We can't log errors yet, so we use eprintln! for early failures
    let config = match Config::from_json_file(&args.config) {
        Ok(mut cfg) => {
            // Validate mod configuration and build mod lists
            // Pass custom_home to resolve mods path correctly
            if let Err(e) = cfg.validate_mods(args.home.as_deref()) {
                eprintln!("Configuration validation error: {}", e);
                std::process::exit(1);
            }
            cfg
        }
        Err(e) => {
            eprintln!("Failed to load config from '{}': {}", args.config, e);
            std::process::exit(1);
        }
    };

    // Parse log level from config
    let log_level = match config.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => {
            eprintln!("Warning: Invalid log level '{}', using INFO", config.log_level);
            Level::INFO
        }
    };

    // Setup logging with configured log level
    let log_config = if args.log_file {
        let file = std::fs::File::create("stam_server.log")
            .expect("Unable to create stam_server.log");
        LogConfig::new("stam_server::")
            .with_level(log_level)
            .with_log_file(file)
    } else {
        LogConfig::<std::fs::File>::new("stam_server::")
            .with_level(log_level)
    };

    init_logging(log_config).expect("Failed to initialize logging");

    info!("Staminal Core Server v{}", VERSION);
    info!("Copyright (C) 2025 Magius(CHE)");
    info!("Configuration: {}", args.config);

    debug!("Settings:");
    debug!("  Local IP: {}", config.local_ip);
    debug!("  Local Port: {}", config.local_port);
    debug!("  Mods Path: {}", config.mods_path);
    debug!("  Tick Rate: {} Hz", config.tick_rate);
    debug!("  Log Level: {}", config.log_level);

    // Setup shutdown flag early (used by JS runtimes and signal handlers)
    let shutdown = Arc::new(AtomicBool::new(false));

    // 1. Initialize mod system (validate + load server-side mods)
    let mod_runtimes =
        match mod_loader::initialize_all_games(&config, VERSION, args.home.as_deref()) {
            Ok(runtime) => runtime,
            Err(e) => {
                error!("Failed to initialize mods. {}", e);
                return;
            }
        };

    let total_server_mods: usize = mod_runtimes.values().map(|r| r.server_mods.len()).sum();
    let total_client_mods: usize = mod_runtimes.values().map(|r| r.client_mods.len()).sum();
    info!(
        "Validated client mods: {}, loaded server mods: {}",
        total_client_mods, total_server_mods
    );

    // Wrap mod_runtimes in Arc for sharing with PrimalClient handlers
    let game_runtimes = Arc::new(mod_runtimes);

    // Spawn JS event loops for any game that has server-side JS mods
    for (game_id, runtime) in game_runtimes.iter() {
        if let Some(js_runtime) = runtime.js_runtime.clone() {
            let gid = game_id.clone();
            let shutdown_token = shutdown.clone();
            tokio::spawn(async move {
                info!("Running JS event loop for game '{}'", gid);
                let mut js_loop = std::pin::pin!(run_js_event_loop(js_runtime));
                let shutdown_for_wait = shutdown_token.clone();
                tokio::select! {
                    fatal_error = &mut js_loop => {
                        if fatal_error {
                            error!("Fatal JavaScript error in game '{}', mod event loop terminated", gid);
                            // Signal main loop to shutdown gracefully
                            shutdown_token.store(true, Ordering::Relaxed);
                        }
                    },
                    _ = wait_for_shutdown(shutdown_for_wait) => {},
                }
            });
        }
    }

    // 2. Avvio TCP Listener for Primal Clients
    let bind_addr = format!("{}:{}", config.local_ip, config.local_port);
    info!("Binding TCP on {}...", bind_addr);

    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(listener) => {
            info!("TCP listener started successfully");
            listener
        }
        Err(e) => {
            error!("Failed to bind TCP listener on {}: {}", bind_addr, e);
            error!("Cannot start server without network listener");
            return;
        }
    };

    // Check if any game has registered TerminalKeyPressed handlers
    let mut total_terminal_handlers = 0;
    for runtime in game_runtimes.values() {
        total_terminal_handlers += runtime.terminal_key_handler_count().await;
    }

    // Show main loop message, with or without Ctrl+C hint depending on handler registration
    if total_terminal_handlers > 0 {
        info!("Entering Main Loop. Waiting for intents...");
    } else {
        info!("Entering Main Loop. Waiting for intents...(Use Ctrl+C to save & shutdown)");
    }

    // Create client manager for tracking active connections
    let client_manager = ClientManager::new();

    // Collect shutdown receivers from all game runtimes and aggregate them
    // into a single channel for the main loop
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<i32>(1);
    for (game_id, runtime) in game_runtimes.iter() {
        if let Some(mut game_shutdown_rx) = runtime.take_shutdown_receiver().await {
            let tx = shutdown_tx.clone();
            let gid = game_id.clone();
            tokio::spawn(async move {
                if let Some(request) = game_shutdown_rx.recv().await {
                    info!("Shutdown requested by mod in game '{}' with exit code {}", gid, request.exit_code);
                    let _ = tx.send(request.exit_code).await;
                }
            });
        }
    }
    // Drop the original sender so the channel closes when all game senders are done
    drop(shutdown_tx);

    // Collect send_event receivers from all game runtimes and aggregate them
    // into a single channel for the main loop (with game_id for proper dispatch)
    let (send_event_tx, mut send_event_rx) = tokio::sync::mpsc::channel::<(String, stam_mod_runtimes::api::SendEventRequest)>(16);
    for (game_id, runtime) in game_runtimes.iter() {
        if let Some(mut game_send_event_rx) = runtime.take_send_event_receiver().await {
            let tx = send_event_tx.clone();
            let gid = game_id.clone();
            tokio::spawn(async move {
                while let Some(request) = game_send_event_rx.recv().await {
                    trace!("send_event request from game '{}': event='{}'", gid, request.event_name);
                    if tx.send((gid.clone(), request)).await.is_err() {
                        break;
                    }
                }
            });
        }
    }
    // Drop the original sender so the channel closes when all game senders are done
    drop(send_event_tx);

    // Start terminal input reader if running in a terminal
    let (mut terminal_rx, mut terminal_handle) = if stam_mod_runtimes::terminal_input::is_terminal() {
        match stam_mod_runtimes::terminal_input::spawn_terminal_event_reader() {
            Ok((rx, handle)) => {
                debug!("Terminal input reader started");
                (Some(rx), Some(handle))
            }
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

    // Setup SIGTERM handler (Linux/Unix only) - this one remains separate since it's a different signal
    #[cfg(unix)]
    {
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            match signal::unix::signal(signal::unix::SignalKind::terminate()) {
                Ok(mut stream) => {
                    stream.recv().await;
                    info!("Received SIGTERM signal");
                    shutdown_clone.store(true, Ordering::Relaxed);
                }
                Err(err) => {
                    warn!("Error setting up SIGTERM handler: {}", err);
                }
            }
        });
    }

    // 3. Main Loop (Game Loop + TCP Accept + Signal Handling)
    let tick_duration = Duration::from_millis(1000 / config.tick_rate);
    let mut tick_interval = interval(tick_duration);

    loop {
        tokio::select! {
            biased;

            // Handle shutdown requests from mods (system.exit)
            exit_code = shutdown_rx.recv(), if !shutdown_rx.is_closed() => {
                if let Some(code) = exit_code {
                    info!("Graceful shutdown requested with exit code {}", code);
                    // TODO: Could store exit_code and use it when process exits
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
                    trace!("Terminal key received: key='{}', ctrl={}, combo='{}'",
                        key_request.key, key_request.ctrl, key_request.combo);

                    // Dispatch to all game runtimes
                    let mut handled = false;
                    for (game_id, runtime) in game_runtimes.iter() {
                        let response = runtime.dispatch_terminal_key(&key_request).await;
                        if response.handled {
                            debug!("Key '{}' handled by mod in game '{}'", key_request.combo, game_id);
                            handled = true;
                            break;
                        }
                    }

                    // Check for Ctrl+C - default exit behavior
                    if !handled && key_request.ctrl && key_request.key == "c" {
                        info!("Received shutdown signal (Ctrl+C)");
                        break;
                    }
                }
            }

            // Handle send_event requests from JavaScript mods
            // This implements the channel-based dispatch pattern described in docs/event-system.md
            request = send_event_rx.recv() => {
                if let Some((game_id, request)) = request {
                    trace!("Dispatching custom event '{}' for game '{}'", request.event_name, game_id);

                    // Get the runtime for this game and dispatch the event
                    let response = if let Some(runtime) = game_runtimes.get(&game_id) {
                        let event_request = stam_mod_runtimes::api::CustomEventRequest::new(
                            request.event_name.clone(),
                            request.args.clone(),
                        );
                        runtime.dispatch_custom_event(&event_request).await
                    } else {
                        warn!("Game runtime '{}' not found for send_event", game_id);
                        stam_mod_runtimes::api::CustomEventResponse::default()
                    };

                    // Send response back to the calling JS code
                    let _ = request.response_tx.send(response);
                }
            }

            // Fallback Ctrl+C handler when terminal input is not available
            _ = async {
                if !terminal_input_active {
                    signal::ctrl_c().await.ok();
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("Received shutdown signal (Ctrl+C)");
                break;
            }

            // Handle tick for game loop
            _ = tick_interval.tick() => {
                // Check shutdown (from SIGTERM handler)
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                // In un vero engine, qui calcoleremmo il "Delta Time"
                // Simula lavoro del server
                // server.process_packets();
            }

            // Handle incoming TCP connections
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        info!("Accepted connection from {}", addr);

                        // Clone config, client_manager, and game_runtimes for the spawned task
                        let config_clone = config.clone();
                        let client_manager_clone = client_manager.clone();
                        let game_runtimes_clone = Arc::clone(&game_runtimes);

                        // Spawn a task to handle this client
                        tokio::spawn(async move {
                            let client = PrimalClient::new(stream, addr, config_clone, client_manager_clone, game_runtimes_clone);
                            client.handle().await;
                        });
                    }
                    Err(e) => {
                        error!("Error accepting connection: {}", e);
                    }
                }
            }
        }
    }

    // Stop terminal input reader and wait for cleanup to complete
    // This ensures raw mode is properly disabled before the process exits
    if let Some(ref mut handle) = terminal_handle {
        handle.stop_async().await;
    }

    info!("Shutting down server gracefully...");

    // Disconnect all active clients with locale ID
    client_manager
        .disconnect_all("disconnect-server-shutdown")
        .await;

    // Give clients time to receive disconnect message before closing
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // TODO: Cleanup resources, save state, etc.
    info!("Shutdown complete.");
}
