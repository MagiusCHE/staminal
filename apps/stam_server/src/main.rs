use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::time::{Duration, interval};
use tracing::{Level, debug, error, info, warn};

use stam_mod_runtimes::adapters::js::run_js_event_loop;
use stam_mod_runtimes::logging::{create_custom_timer, CustomFormatter};
use stam_schema::Validatable;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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

    // Load and validate configuration
    let config = match Config::from_json_file(&args.config) {
        Ok(mut cfg) => {
            // Validate mod configuration and build mod lists
            if let Err(e) = cfg.validate_mods() {
                eprintln!("Configuration validation error: {}", e);
                std::process::exit(1);
            }
            cfg
        },
        Err(e) => {
            eprintln!("Failed to load config from '{}': {}", args.config, e);
            eprintln!("Using default configuration");
            Config::default()
        }
    };

    // Parse log level from string
    let log_level = match config.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => {
            eprintln!("Invalid log level '{}', using INFO", config.log_level);
            Level::INFO
        }
    };

    // Custom time format: YYYY/MM/DD hh:mm:ss.xxxx
    let timer = create_custom_timer();

    // Auto-detect if stdout is a terminal for ANSI color support
    let use_ansi = atty::is(atty::Stream::Stdout)
        && std::env::var("NO_COLOR").is_err()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true);

    // Setup logging based on whether file logging is enabled
    if args.log_file {
        // File logging: no ANSI colors, truncate previous run
        let file = std::fs::File::create("stam_server.log")
            .expect("Unable to create stam_server.log");
        let formatter_stdout = CustomFormatter::new(timer.clone(), use_ansi)
            .with_strip_prefix("stam_server::");
        let formatter_file = CustomFormatter::new(timer, false)
            .with_strip_prefix("stam_server::");

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_stdout)
                    .with_ansi(use_ansi)
                    .with_writer(std::io::stdout),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_file)
                    .with_ansi(false)
                    .with_writer(file),
            )
            .with(tracing_subscriber::filter::LevelFilter::from_level(log_level))
            .init();
    } else {
        let formatter = CustomFormatter::new(timer, use_ansi)
            .with_strip_prefix("stam_server::");
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter)
                    .with_ansi(use_ansi)
                    .with_writer(std::io::stdout),
            )
            .with(tracing_subscriber::filter::LevelFilter::from_level(log_level))
            .init();
    }

    info!("========================================");
    info!("   STAMINAL CORE SERVER v{}", VERSION);
    info!("   State: Undifferentiated");
    info!("========================================");
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
    let mod_runtimes = match mod_loader::initialize_all_games(&config, VERSION, args.home.as_deref()) {
        Ok(runtime) => runtime,
        Err(e) => {
            error!("Failed to initialize mods. {}", e);
            return;
        }
    };

    let total_server_mods: usize = mod_runtimes.values().map(|r| r.server_mods.len()).sum();
    let total_client_mods: usize = mod_runtimes.values().map(|r| r.client_mods.len()).sum();
    info!("Validated client mods: {}, loaded server mods: {}", total_client_mods, total_server_mods);

    // Spawn JS event loops for any game that has server-side JS mods
    for (game_id, runtime) in &mod_runtimes {
        if let Some(js_runtime) = runtime.js_runtime.clone() {
            let gid = game_id.clone();
            let shutdown_token = shutdown.clone();
            tokio::spawn(async move {
                info!("Running JS event loop for game '{}'", gid);
                let mut js_loop = std::pin::pin!(run_js_event_loop(js_runtime));
                tokio::select! {
                    _ = &mut js_loop => {},
                    _ = wait_for_shutdown(shutdown_token) => {},
                }
            });
        }
    }

    // 2. Avvio TCP Listener for Primal Clients
    let bind_addr = format!("{}:{}", config.local_ip, config.local_port);
    info!("[NET] Binding TCP on {}...", bind_addr);

    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(listener) => {
            info!("[NET] TCP listener started successfully");
            listener
        }
        Err(e) => {
            error!("[NET] Failed to bind TCP listener on {}: {}", bind_addr, e);
            error!("[CORE] Cannot start server without network listener");
            return;
        }
    };

    info!("[CORE] Entering Main Loop. Waiting for intents...(Use Ctrl+C to save & shutdown)");

    // Create client manager for tracking active connections
    let client_manager = ClientManager::new();
    let shutdown_clone = shutdown.clone();

    // Spawn signal handler task
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("[CORE] Received shutdown signal (Ctrl+C)");
                shutdown_clone.store(true, Ordering::Relaxed);
            }
            Err(err) => {
                warn!("[CORE] Error listening for shutdown signal: {}", err);
            }
        }
    });

    // Setup SIGTERM handler (Linux/Unix only)
    #[cfg(unix)]
    {
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            match signal::unix::signal(signal::unix::SignalKind::terminate()) {
                Ok(mut stream) => {
                    stream.recv().await;
                    info!("[CORE] Received SIGTERM signal");
                    shutdown_clone.store(true, Ordering::Relaxed);
                }
                Err(err) => {
                    warn!("[CORE] Error setting up SIGTERM handler: {}", err);
                }
            }
        });
    }

    // 3. Main Loop (Game Loop + TCP Accept)
    let tick_duration = Duration::from_millis(1000 / config.tick_rate);
    let mut tick_interval = interval(tick_duration);

    loop {
        tokio::select! {
            // Handle tick for game loop
            _ = tick_interval.tick() => {
                // Check shutdown
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
                        info!("[NET] Accepted connection from {}", addr);

                        // Clone config and client_manager for the spawned task
                        let config_clone = config.clone();
                        let client_manager_clone = client_manager.clone();

                        // Spawn a task to handle this client
                        tokio::spawn(async move {
                            let client = PrimalClient::new(stream, addr, config_clone, client_manager_clone);
                            client.handle().await;
                        });
                    }
                    Err(e) => {
                        error!("[NET] Error accepting connection: {}", e);
                    }
                }
            }
        }
    }

    info!("[CORE] Shutting down server gracefully...");

    // Disconnect all active clients with locale ID
    client_manager.disconnect_all("disconnect-server-shutdown").await;

    // Give clients time to receive disconnect message before closing
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // TODO: Cleanup resources, save state, etc.
    info!("[CORE] Shutdown complete.");
}
