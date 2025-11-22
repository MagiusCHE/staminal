use std::thread;
use std::time::Duration;
use std::env;

use clap::Parser;
use tracing::{Level, info, debug};
use tracing_subscriber::FmtSubscriber;
use tracing_subscriber::fmt::time::OffsetTime;
use time::macros::format_description;

use stam_schema::Validatable;

mod config;
use config::Config;

const VERSION: &str = "0.1.0-alpha";

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
}

fn main() {
    let args = Args::parse();

    // Load and validate configuration
    let config = match Config::from_json_file(&args.config) {
        Ok(cfg) => cfg,
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
    let timer = OffsetTime::new(
        time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
        format_description!("[year]/[month]/[day] [hour]:[minute]:[second].[subsecond digits:4]"),
    );

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_timer(timer)
        .with_thread_ids(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    info!("========================================");
    info!("   STAMINAL CORE SERVER v{}", VERSION);
    info!("   State: Undifferentiated");
    info!("========================================");
    info!("Configuration: {}", args.config);

    debug!("Settings:");
    debug!("  Host: {}", config.host);
    debug!("  Port: {}", config.port);
    debug!("  Mods Path: {}", config.mods_path);
    debug!("  Tick Rate: {} Hz", config.tick_rate);
    debug!("  Log Level: {}", config.log_level);

    // 1. Inizializzazione Mod Loader (Placeholder)
    info!("[CORE] Scanning '{}' for DNA...", config.mods_path);
    let mods_found = 0;
    // TODO: Implementare scansione directory mods_path
    info!("[CORE] Found {} potential differentiations.", mods_found);

    // 2. Avvio Networking (Placeholder TCP/UDP)
    let bind_addr = format!("{}:{}", config.host, config.port);
    info!("[NET] Binding UDP on {}...", bind_addr);
    // let server = Server::bind(&bind_addr).unwrap();

    info!("[CORE] Entering Main Loop. Waiting for intents...");

    // 3. Main Loop (Game Loop)
    let tick_duration = Duration::from_millis(1000 / config.tick_rate);
    loop {
        // In un vero engine, qui calcoleremmo il "Delta Time"

        // Simula lavoro del server
        // server.process_packets();

        // Mantieni il tick rate stabile
        thread::sleep(tick_duration);
        break; // Rimuovere questo break in un vero server
    }
    info!("[CORE] Shutting down server.");
}
