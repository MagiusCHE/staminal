use clap::Parser;
use tracing::{Level, info, error, warn};
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::fmt::{self, format::Writer, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::registry::LookupSpan;
use time::macros::format_description;
use std::fmt as std_fmt;
use tokio::net::TcpStream;
use sha2::{Sha512, Digest};

use stam_protocol::{PrimalMessage, PrimalStream, IntentType};

const VERSION: &str = "0.1.0-alpha";

/// Compute SHA-512 hash of a string and return as hex string
fn sha512_hash(input: &str) -> String {
    let mut hasher = Sha512::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Custom event formatter that displays thread IDs as #N instead of ThreadId(N)
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

        let (dim_start, dim_end) = if self.ansi { ("\x1b[2m", "\x1b[0m") } else { ("", "") };
        let (level_color, level_str) = match *metadata.level() {
            Level::ERROR => (if self.ansi { "\x1b[31m" } else { "" }, "ERROR"),
            Level::WARN  => (if self.ansi { "\x1b[33m" } else { "" }, " WARN"),
            Level::INFO  => (if self.ansi { "\x1b[32m" } else { "" }, " INFO"),
            Level::DEBUG => (if self.ansi { "\x1b[34m" } else { "" }, "DEBUG"),
            Level::TRACE => (if self.ansi { "\x1b[35m" } else { "" }, "TRACE"),
        };
        let color_end = if self.ansi { "\x1b[0m" } else { "" };

        write!(writer, "{}", dim_start)?;
        self.timer.format_time(&mut writer)?;
        write!(writer, "{} ", dim_end)?;

        write!(writer, "{}{}{} ", level_color, level_str, color_end)?;

        let thread_id = format!("{:?}", std::thread::current().id());
        if let Some(num_str) = thread_id.strip_prefix("ThreadId(").and_then(|s| s.strip_suffix(")")) {
            if let Ok(num) = num_str.parse::<u64>() {
                write!(writer, "#{:03} ", num)?;
            }
        }

        write!(writer, "{}{}{}: ", dim_start, metadata.target(), dim_end)?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
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
}

#[tokio::main]
async fn main() {
    // Setup logging
    let timer = OffsetTime::new(
        time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
        format_description!("[year]/[month]/[day] [hour]:[minute]:[second].[subsecond digits:4]"),
    );

    let use_ansi = atty::is(atty::Stream::Stdout);
    let formatter = CustomFormatter { timer, ansi: use_ansi };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .event_format(formatter)
                .with_writer(std::io::stdout)
        )
        .with(tracing_subscriber::filter::LevelFilter::from_level(Level::DEBUG))
        .init();

    let args = Args::parse();

    info!("========================================");
    info!("   STAMINAL CLIENT v{}", VERSION);
    info!("========================================");

    // Parse URI
    if !args.uri.starts_with("stam://") {
        error!("Invalid URI scheme. Expected 'stam://', got: {}", args.uri);
        return;
    }

    let uri_without_scheme = args.uri.strip_prefix("stam://").unwrap();

    // Parse username:password@host:port
    let (credentials, host_port) = if let Some(at_pos) = uri_without_scheme.find('@') {
        let creds = &uri_without_scheme[..at_pos];
        let host = &uri_without_scheme[at_pos + 1..];
        (Some(creds), host)
    } else {
        error!("URI must include credentials: stam://username:password@host:port");
        return;
    };

    let (username, password) = if let Some(creds) = credentials {
        if let Some(colon_pos) = creds.find(':') {
            let user = &creds[..colon_pos];
            let pass = &creds[colon_pos + 1..];
            (user.to_string(), pass.to_string())
        } else {
            error!("Credentials must be in format username:password");
            return;
        }
    } else {
        error!("Missing credentials");
        return;
    };

    info!("Connecting to {} as user '{}'", host_port, username);

    // Connect to server
    let mut stream = match TcpStream::connect(host_port).await {
        Ok(s) => {
            info!("Connected to {}", host_port);
            s
        }
        Err(e) => {
            error!("Failed to connect to {}: {}", host_port, e);
            return;
        }
    };

    // Read Welcome message
    match stream.read_primal_message().await {
        Ok(PrimalMessage::Welcome { version }) => {
            info!("Received Welcome from server, version: {}", version);

            // Check version compatibility (major.minor must match)
            let client_version_parts: Vec<&str> = VERSION.split('.').collect();
            let server_version_parts: Vec<&str> = version.split('.').collect();

            if client_version_parts.len() >= 2 && server_version_parts.len() >= 2 {
                let client_major_minor = format!("{}.{}", client_version_parts[0], client_version_parts[1]);
                let server_major_minor = format!("{}.{}", server_version_parts[0], server_version_parts[1]);

                if client_major_minor != server_major_minor {
                    error!("Version mismatch! Client: {}, Server: {}", VERSION, version);
                    error!("Major.minor versions must match");
                    return;
                }

                info!("Version compatible: {} ~ {}", VERSION, version);
            }
        }
        Ok(msg) => {
            error!("Expected Welcome message, got: {:?}", msg);
            return;
        }
        Err(e) => {
            error!("Failed to read Welcome: {}", e);
            return;
        }
    }

    // Send Intent with PrimalLogin
    info!("Sending PrimalLogin intent...");

    // Hash password with SHA-512
    let password_hash = sha512_hash(&password);

    let intent = PrimalMessage::Intent {
        intent_type: IntentType::PrimalLogin,
        client_version: VERSION.to_string(),
        username: username.clone(),
        password_hash,
    };

    if let Err(e) = stream.write_primal_message(&intent).await {
        error!("Failed to send Intent: {}", e);
        return;
    }

    // Wait for ServerList or Error
    match stream.read_primal_message().await {
        Ok(PrimalMessage::ServerList { servers }) => {
            info!("Received server list with {} server(s)", servers.len());

            if servers.is_empty() {
                warn!("Server list is empty, no game servers available");
                return;
            }

            for (i, server) in servers.iter().enumerate() {
                info!("  [{}] {} - {}", i + 1, server.name, server.uri);
            }

            // Connect to first server in list
            let first_server = &servers[0];
            info!("Attempting to connect to game server: {} ({})", first_server.name, first_server.uri);

            // TODO: Parse game server URI and connect
            // For now, just wait
            info!("Game client connection not yet implemented");
            info!("Client will now wait for Ctrl+C...");

            // Wait for shutdown signal
            tokio::signal::ctrl_c().await.ok();
            info!("Shutting down client...");
        }
        Ok(PrimalMessage::Error { message }) => {
            error!("Server error: {}", message);
        }
        Ok(msg) => {
            error!("Expected ServerList or Error, got: {:?}", msg);
        }
        Err(e) => {
            error!("Failed to read server response: {}", e);
        }
    }
}
