use clap::Parser;
use tracing::{Level, info, error, warn, debug};
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::fmt::{self, format::Writer, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::registry::LookupSpan;
use time::macros::format_description;
use std::fmt as std_fmt;
use tokio::net::TcpStream;
use sha2::{Sha512, Digest};

use stam_protocol::{PrimalMessage, PrimalStream, IntentType, GameMessage, GameStream};

#[macro_use]
mod locale;
use locale::LocaleManager;

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

/// Connect to game server and maintain connection
async fn connect_to_game_server(uri: &str, username: &str, password: &str, locale: &LocaleManager) -> Result<(), Box<dyn std::error::Error>> {
    // Parse game server URI (stam://host:port)
    if !uri.starts_with("stam://") {
        return Err(locale.get_with_args("error-invalid-uri", Some(&fluent_args!{
            "uri" => uri
        })).into());
    }

    let host_port = uri.strip_prefix("stam://").unwrap();
    info!("{}", locale.get_with_args("game-connecting", Some(&fluent_args!{
        "host" => host_port
    })));

    // Connect to game server
    let mut stream = TcpStream::connect(host_port).await?;
    info!("{}", locale.get("game-connected"));

    // Read Welcome message
    match stream.read_primal_message().await {
        Ok(PrimalMessage::Welcome { version }) => {
            info!("{}", locale.get_with_args("server-welcome", Some(&fluent_args!{
                "version" => version.as_str()
            })));

            // Check version compatibility
            let client_version_parts: Vec<&str> = VERSION.split('.').collect();
            let server_version_parts: Vec<&str> = version.split('.').collect();

            if client_version_parts.len() >= 2 && server_version_parts.len() >= 2 {
                let client_major_minor = format!("{}.{}", client_version_parts[0], client_version_parts[1]);
                let server_major_minor = format!("{}.{}", server_version_parts[0], server_version_parts[1]);

                if client_major_minor != server_major_minor {
                    error!("{}", locale.get_with_args("version-mismatch", Some(&fluent_args!{
                        "client" => VERSION,
                        "server" => version.as_str()
                    })));
                    return Err(locale.get("disconnect-version-mismatch").into());
                }

                info!("{}", locale.get_with_args("version-compatible", Some(&fluent_args!{
                    "client" => VERSION,
                    "server" => version.as_str()
                })));
            }
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
    };

    stream.write_primal_message(&intent).await?;

    // Wait for LoginSuccess
    match stream.read_game_message().await {
        Ok(GameMessage::LoginSuccess) => {
            info!("{}", locale.get("game-login-success"));
        }
        Ok(GameMessage::Error { message }) => {
            // Message from server could be a locale ID
            let localized_msg = locale.get(&message);
            error!("{}", locale.get_with_args("server-error", Some(&fluent_args!{
                "message" => localized_msg.as_str()
            })));
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

    tokio::select! {
        _ = maintain_game_connection(&mut stream, locale) => {
            info!("{}", locale.get("connection-closed"));
        }
        _ = tokio::signal::ctrl_c() => {
            info!("{}", locale.get("ctrl-c-received"));
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
                error!("{}", locale.get_with_args("server-error", Some(&fluent_args!{
                    "message" => localized_msg.as_str()
                })));
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

    // Initialize locale manager
    let locale = match LocaleManager::new(&args.assets, args.lang.as_deref()) {
        Ok(lm) => {
            info!("Locale system initialized: {}", lm.current_locale());
            lm
        }
        Err(e) => {
            error!("Failed to initialize locale system: {}", e);
            error!("Continuing without localization support");
            return;
        }
    };

    // Parse URI
    if !args.uri.starts_with("stam://") {
        error!("{}", locale.get_with_args("error-invalid-uri", Some(&fluent_args!{
            "uri" => args.uri.as_str()
        })));
        return;
    }

    let uri_without_scheme = args.uri.strip_prefix("stam://").unwrap();

    // Parse username:password@host:port
    let (credentials, host_port) = if let Some(at_pos) = uri_without_scheme.find('@') {
        let creds = &uri_without_scheme[..at_pos];
        let host = &uri_without_scheme[at_pos + 1..];
        (Some(creds), host)
    } else {
        error!("{}", locale.get_with_args("error-invalid-uri", Some(&fluent_args!{
            "uri" => args.uri.as_str()
        })));
        return;
    };

    let (username, password) = if let Some(creds) = credentials {
        if let Some(colon_pos) = creds.find(':') {
            let user = &creds[..colon_pos];
            let pass = &creds[colon_pos + 1..];
            (user.to_string(), pass.to_string())
        } else {
            error!("{}", locale.get_with_args("error-invalid-uri", Some(&fluent_args!{
                "uri" => args.uri.as_str()
            })));
            return;
        }
    } else {
        error!("{}", locale.get_with_args("error-invalid-uri", Some(&fluent_args!{
            "uri" => args.uri.as_str()
        })));
        return;
    };

    info!("{}", locale.get_with_args("connecting", Some(&fluent_args!{
        "host" => host_port
    })));

    // Connect to server
    let mut stream = match TcpStream::connect(host_port).await {
        Ok(s) => {
            info!("{}", locale.get_with_args("connected", Some(&fluent_args!{
                "host" => host_port
            })));
            s
        }
        Err(e) => {
            error!("{}", locale.get_with_args("connection-failed", Some(&fluent_args!{
                "error" => e.to_string().as_str()
            })));
            return;
        }
    };

    // Read Welcome message
    match stream.read_primal_message().await {
        Ok(PrimalMessage::Welcome { version }) => {
            info!("{}", locale.get_with_args("server-welcome", Some(&fluent_args!{
                "version" => version.as_str()
            })));

            // Check version compatibility (major.minor must match)
            let client_version_parts: Vec<&str> = VERSION.split('.').collect();
            let server_version_parts: Vec<&str> = version.split('.').collect();

            if client_version_parts.len() >= 2 && server_version_parts.len() >= 2 {
                let client_major_minor = format!("{}.{}", client_version_parts[0], client_version_parts[1]);
                let server_major_minor = format!("{}.{}", server_version_parts[0], server_version_parts[1]);

                if client_major_minor != server_major_minor {
                    error!("{}", locale.get_with_args("version-mismatch", Some(&fluent_args!{
                        "client" => VERSION,
                        "server" => version.as_str()
                    })));
                    return;
                }

                info!("{}", locale.get_with_args("version-compatible", Some(&fluent_args!{
                    "client" => VERSION,
                    "server" => version.as_str()
                })));
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
    };

    if let Err(e) = stream.write_primal_message(&intent).await {
        error!("{}: {}", locale.get("login-failed"), e);
        return;
    }

    // Wait for ServerList or Error
    match stream.read_primal_message().await {
        Ok(PrimalMessage::ServerList { servers }) => {
            info!("{}", locale.get_with_args("server-list-received", Some(&fluent_args!{
                "count" => servers.len()
            })));

            if servers.is_empty() {
                warn!("{}", locale.get("server-list-empty"));
                return;
            }

            for (i, server) in servers.iter().enumerate() {
                info!("  [{}] {} - {}", i + 1, server.name, server.uri);
            }

            // Connect to first server in list
            let first_server = &servers[0];
            info!("Attempting to connect to game server: {} ({})", first_server.name, first_server.uri);

            // Parse game server URI and connect
            if let Err(e) = connect_to_game_server(&first_server.uri, &username, &password, &locale).await {
                error!("{}: {}", locale.get("connection-failed"), e);
            }
        }
        Ok(PrimalMessage::Error { message }) => {
            // Message could be a locale ID
            let localized_msg = locale.get(&message);
            error!("{}", locale.get_with_args("server-error", Some(&fluent_args!{
                "message" => localized_msg.as_str()
            })));
        }
        Ok(msg) => {
            error!("{}: {:?}", locale.get("error-unexpected-message"), msg);
        }
        Err(e) => {
            error!("{}: {}", locale.get("error-parse-failed"), e);
        }
    }
}
