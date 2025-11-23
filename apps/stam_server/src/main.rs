use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use tracing::{Level, info, debug, warn, error};
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::fmt::{self, format::Writer, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::registry::LookupSpan;
use time::macros::format_description;
use std::fmt as std_fmt;
use tokio::time::{Duration, interval};
use tokio::signal;
use tokio::net::TcpListener;

use stam_schema::Validatable;

mod config;
use config::Config;

mod primal_client;
use primal_client::PrimalClient;

mod game_client;

mod client_manager;
use client_manager::ClientManager;

const VERSION: &str = "0.1.0-alpha";

/// Custom event formatter that displays thread IDs as #N instead of ThreadId(N)
/// Replicates the default tracing formatter behavior with ANSI color support
struct CustomFormatter<T> {
    timer: T,
    ansi: bool,
}

impl<S, N, T> FormatEvent<S, N> for CustomFormatter<T>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: fmt::time::FormatTime,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std_fmt::Result {
        let metadata = event.metadata();

        // Color codes (matching tracing's default colors)
        let (dim_start, dim_end) = if self.ansi { ("\x1b[2m", "\x1b[0m") } else { ("", "") };
        let (level_color, level_str) = match *metadata.level() {
            Level::ERROR => (if self.ansi { "\x1b[31m" } else { "" }, "ERROR"),
            Level::WARN  => (if self.ansi { "\x1b[33m" } else { "" }, " WARN"),
            Level::INFO  => (if self.ansi { "\x1b[32m" } else { "" }, " INFO"),
            Level::DEBUG => (if self.ansi { "\x1b[34m" } else { "" }, "DEBUG"),
            Level::TRACE => (if self.ansi { "\x1b[35m" } else { "" }, "TRACE"),
        };
        let color_end = if self.ansi { "\x1b[0m" } else { "" };

        // Write timestamp (dimmed)
        write!(writer, "{}", dim_start)?;
        self.timer.format_time(&mut writer)?;
        write!(writer, "{} ", dim_end)?;

        // Write level (colored)
        write!(writer, "{}{}{} ", level_color, level_str, color_end)?;

        // Write thread ID as #N instead of ThreadId(N)
        let thread_id = format!("{:?}", std::thread::current().id());
        if let Some(num_str) = thread_id.strip_prefix("ThreadId(").and_then(|s| s.strip_suffix(")")) {
            // Parse to number for proper padding
            if let Ok(num) = num_str.parse::<u64>() {
                write!(writer, "#{:03} ", num)?;
            }
        }

        // Write target (dimmed)
        write!(writer, "{}{}{}: ", dim_start, metadata.target(), dim_end)?;

        // Write fields
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
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
}

#[tokio::main]
async fn main() {
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

    // Auto-detect if stdout is a terminal for ANSI color support
    let use_ansi = atty::is(atty::Stream::Stdout);

    // Create custom formatter with #N thread IDs
    let formatter = CustomFormatter {
        timer,
        ansi: use_ansi,
    };

    // Build subscriber with custom formatter
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .event_format(formatter)
                .with_writer(std::io::stdout)
        )
        .with(tracing_subscriber::filter::LevelFilter::from_level(log_level))
        .init();

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

    // 1. Inizializzazione Mod Loader (Placeholder)
    info!("[CORE] Scanning '{}' for DNA...", config.mods_path);
    let mods_found = 0;
    // TODO: Implementare scansione directory mods_path
    info!("[CORE] Found {} potential differentiations.", mods_found);

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

    // Setup shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));
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

    // Disconnect all active clients
    client_manager.disconnect_all("Server is shutting down").await;
    // TODO: Cleanup resources, save state, etc.
    info!("[CORE] Shutdown complete.");
}
