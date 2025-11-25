use clap::Parser;
use tracing::{Level, info, error, warn, debug};
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::fmt::{self, format::Writer, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::field::Visit;
use tracing::field::Field;
use time::macros::format_description;
use std::fmt as std_fmt;
use std::path::PathBuf;
use std::fs;
use tokio::net::TcpStream;
use sha2::{Sha512, Digest};
use semver::Version;
use serde::Deserialize;

use stam_protocol::{PrimalMessage, PrimalStream, IntentType, GameMessage, GameStream};

#[macro_use]
mod locale;
use locale::LocaleManager;

mod app_paths;
mod mod_runtime;

use app_paths::AppPaths;
use mod_runtime::{ModRuntimeManager, JsRuntimeAdapter, JsRuntimeConfig};
use mod_runtime::js_adapter::{create_js_runtime_config, run_js_event_loop};

const VERSION: &str = "0.1.0-alpha";

/// Compute SHA-512 hash of a string and return as hex string
fn sha512_hash(input: &str) -> String {
    let mut hasher = Sha512::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Mod manifest structure
#[derive(Debug, Deserialize)]
struct ModManifest {
    name: String,
    version: String,
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    entry_point: String,
    #[allow(dead_code)]
    priority: i32,
}

/// Validate if a version is within the specified range
/// min_version and max_version should be in format "major.minor.patch"
/// Returns Ok(()) if version is in range, Err with message otherwise
fn validate_version_range(
    mod_id: &str,
    installed_version: &str,
    min_version: &str,
    max_version: &str,
) -> Result<(), String> {
    // Parse installed version
    let installed = Version::parse(installed_version)
        .map_err(|e| format!("Invalid installed version '{}' for mod '{}': {}", installed_version, mod_id, e))?;

    // Parse min and max versions
    let min = Version::parse(min_version)
        .map_err(|e| format!("Invalid min_version '{}' for mod '{}': {}", min_version, mod_id, e))?;

    let max = Version::parse(max_version)
        .map_err(|e| format!("Invalid max_version '{}' for mod '{}': {}", max_version, mod_id, e))?;

    // Check if installed version is within range (inclusive on both ends)
    if installed < min {
        return Err(format!(
            "Mod '{}' version {} is below minimum required version {}",
            mod_id, installed_version, min_version
        ));
    }

    if installed > max {
        return Err(format!(
            "Mod '{}' version {} is above maximum supported version {}",
            mod_id, installed_version, max_version
        ));
    }

    Ok(())
}

/// Visitor to extract runtime_type and mod_id fields
#[derive(Default)]
struct FieldExtractor {
    runtime_type: Option<String>,
    mod_id: Option<String>,
    message: Option<String>,
}

impl Visit for FieldExtractor {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "runtime_type" => self.runtime_type = Some(value.to_string()),
            "mod_id" => self.mod_id = Some(value.to_string()),
            "message" => self.message = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std_fmt::Debug) {
        match field.name() {
            "runtime_type" => self.runtime_type = Some(format!("{:?}", value).trim_matches('"').to_string()),
            "mod_id" => self.mod_id = Some(format!("{:?}", value).trim_matches('"').to_string()),
            "message" => self.message = Some(format!("{:?}", value).trim_matches('"').to_string()),
            _ => {}
        }
    }
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

        // Extract runtime_type and mod_id fields if present
        let mut extractor = FieldExtractor::default();
        event.record(&mut extractor);

        // If both runtime_type and mod_id are present, format as "runtime_type::mod_id:"
        if let (Some(rt), Some(mid)) = (&extractor.runtime_type, &extractor.mod_id) {
            write!(writer, "{}{}::{}{}: ", dim_start, rt, mid, dim_end)?;
            // Print the message if present
            if let Some(msg) = &extractor.message {
                write!(writer, "{}", msg)?;
            }
        } else {
            // Otherwise use the default target formatting
            let target = metadata.target();
            let display_target = target.strip_prefix("stam_client::").unwrap_or(target);
            if !display_target.is_empty() && display_target != "stam_client" {
                write!(writer, "{}{}{}: ", dim_start, display_target, dim_end)?;
            }
            // Use default field formatting
            ctx.field_format().format_fields(writer.by_ref(), event)?;
        }

        writeln!(writer)
    }
}

/// Connect to game server and maintain connection
async fn connect_to_game_server(uri: &str, username: &str, password: &str, game_id: &str, locale: &LocaleManager, app_paths: &AppPaths) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize game-specific directory
    info!("Initializing directories for game '{}'...", game_id);
    let game_root = app_paths.game_root(game_id)?;
    let mods_dir = game_root.join("mods");

    info!("Game directories:");
    info!("  Root: {}", game_root.display());
    info!("  Mods: {}", mods_dir.display());

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
        game_id: Some(game_id.to_string()),
    };

    stream.write_primal_message(&intent).await?;

    // Wait for LoginSuccess
    // JS runtime handle for event loop integration
    let mut js_runtime_handle: Option<std::sync::Arc<stam_mod_runtimes::JsAsyncRuntime>> = None;

    match stream.read_game_message().await {
        Ok(GameMessage::LoginSuccess { mods }) => {
            info!("{}", locale.get("game-login-success"));

            // Log mod list received
            if !mods.is_empty() {
                info!("Received {} required mod(s):", mods.len());
                for mod_info in &mods {
                    info!("  - {} [{}] (v{} - v{}): {}",
                        mod_info.mod_id,
                        mod_info.mod_type,
                        mod_info.min_version,
                        mod_info.max_version,
                        mod_info.download_url
                    );
                }
            } else {
                info!("No mods required for this game");
            }

            // Validate ALL mods are present before continuing
            info!("Validating mods...");

            if !mods.is_empty() {
                info!("Found {} mod(s) to validate", mods.len());

                for mod_info in &mods {
                    let mod_dir = mods_dir.join(&mod_info.mod_id);

                    // Check if mod directory exists
                    if !mod_dir.exists() {
                        error!("Mod '{}' not found in {}",
                            mod_info.mod_id,
                            mod_dir.display()
                        );
                        error!("Required mods must be present before the client can start");
                        error!("TODO: Automatic download will be implemented in the future");
                        return Err(format!(
                            "Missing required mod: {} (expected at {})",
                            mod_info.mod_id,
                            mod_dir.display()
                        ).into());
                    }

                    info!("  ✓ {} [{}] found", mod_info.mod_id, mod_info.mod_type);

                    // Read and validate mod version
                    let manifest_path = mod_dir.join("manifest.json");
                    if !manifest_path.exists() {
                        error!("Mod '{}' missing manifest.json", mod_info.mod_id);
                        return Err(format!(
                            "Mod '{}' is missing manifest.json file",
                            mod_info.mod_id
                        ).into());
                    }

                    let manifest_content = fs::read_to_string(&manifest_path)
                        .map_err(|e| format!("Failed to read manifest for mod '{}': {}", mod_info.mod_id, e))?;

                    let manifest: ModManifest = serde_json::from_str(&manifest_content)
                        .map_err(|e| format!("Failed to parse manifest for mod '{}': {}", mod_info.mod_id, e))?;

                    // Validate version is within required range
                    if let Err(e) = validate_version_range(
                        &mod_info.mod_id,
                        &manifest.version,
                        &mod_info.min_version,
                        &mod_info.max_version,
                    ) {
                        error!("{}", e);
                        error!("Mod version mismatch");
                        error!("TODO: Automatic download/update will be implemented in the future");
                        return Err(e.into());
                    }

                    info!("  ✓ {} version {} OK (required: {} - {})",
                        mod_info.mod_id,
                        manifest.version,
                        mod_info.min_version,
                        mod_info.max_version
                    );
                }

                info!("All mods validated successfully");
            } else {
                info!("No mods required");
            }

            // Initialize mod runtime manager and load ALL mods
            if !mods.is_empty() {
                info!("Initializing mod runtime system...");

                // Create mod runtime manager
                let mut runtime_manager = ModRuntimeManager::new();

                // Initialize JavaScript runtime (one shared runtime for all JS mods)
                info!("Initializing JavaScript runtime...");
                let runtime_config = create_js_runtime_config(&app_paths, &game_id)?;
                let js_adapter = JsRuntimeAdapter::new(runtime_config)?;

                // Get runtime handle BEFORE moving the adapter to the manager
                let js_runtime = js_adapter.get_runtime();

                runtime_manager.register_adapter(stam_mod_runtimes::RuntimeType::JavaScript, Box::new(js_adapter));

                // First pass: Register all mod aliases for cross-mod imports
                // This must happen BEFORE loading any mod, so that import "@mod-id" works
                info!("Registering mod aliases...");
                let mut mod_entry_points: Vec<(String, std::path::PathBuf, String, String)> = Vec::new();

                for mod_info in &mods {
                    let mod_dir = mods_dir.join(&mod_info.mod_id);
                    let manifest_path = mod_dir.join("manifest.json");

                    // Read manifest to get entry_point
                    let manifest_content = fs::read_to_string(&manifest_path)?;
                    let manifest: ModManifest = serde_json::from_str(&manifest_content)?;

                    let entry_point_path = mod_dir.join(&manifest.entry_point);

                    // Convert to absolute path for reliable module resolution
                    let absolute_entry_point = if entry_point_path.is_absolute() {
                        entry_point_path.clone()
                    } else {
                        std::env::current_dir()?.join(&entry_point_path)
                    };

                    // Register alias before loading
                    stam_mod_runtimes::adapters::js::register_mod_alias(
                        &mod_info.mod_id,
                        absolute_entry_point,
                    );
                    info!("  Registered @{}", mod_info.mod_id);

                    mod_entry_points.push((
                        mod_info.mod_id.clone(),
                        entry_point_path,
                        manifest.entry_point.clone(),
                        mod_info.mod_type.clone(),
                    ));
                }

                // Second pass: Load all mods
                info!("Loading mods...");
                for (mod_id, entry_point_path, _entry_point_name, mod_type) in &mod_entry_points {
                    info!("  Loading {} [{}]", mod_id, mod_type);
                    runtime_manager.load_mod(mod_id, entry_point_path)?;
                }

                // Third pass: Call onAttach lifecycle hook for ALL mods
                info!("Attaching mods...");
                for (mod_id, _, _, mod_type) in &mod_entry_points {
                    info!("  Attaching {} [{}]", mod_id, mod_type);
                    runtime_manager.call_mod_function(mod_id, "onAttach")?;
                }

                // Fourth pass: Call onBootstrap ONLY for bootstrap mods
                let bootstrap_mods: Vec<_> = mod_entry_points.iter()
                    .filter(|(_, _, _, mod_type)| mod_type == "bootstrap")
                    .collect();

                if !bootstrap_mods.is_empty() {
                    info!("Bootstrapping mods...");
                    for (mod_id, _, _, _) in &bootstrap_mods {
                        info!("  Bootstrapping {}", mod_id);
                        runtime_manager.call_mod_function(mod_id, "onBootstrap")?;
                    }
                }

                info!("Mod system initialized successfully");
                js_runtime_handle = Some(js_runtime);
            }
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
            _ = run_js_event_loop(js_runtime) => {
                // Event loop shouldn't exit normally
                debug!("JavaScript event loop exited unexpectedly");
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

    /// Custom home directory for Staminal data and config (overrides system directories)
    /// Useful for development and testing
    #[arg(long, env = "STAM_HOME")]
    home: Option<String>,
}

#[tokio::main]
async fn main() {
    // Setup logging
    let timer = OffsetTime::new(
        time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
        format_description!("[year]/[month]/[day] [hour]:[minute]:[second].[subsecond digits:4]"),
    );

    // Disable ANSI colors if:
    // - stdout is not a TTY (piped/redirected)
    // - NO_COLOR env var is set (https://no-color.org/)
    // - TERM=dumb
    let use_ansi = atty::is(atty::Stream::Stdout)
        && std::env::var("NO_COLOR").is_err()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true);
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
        game_id: None,  // Not needed for PrimalLogin
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
                info!("  [{}] {} (game_id: {}) - {}", i + 1, server.name, server.game_id, server.uri);
            }

            // Connect to first server in list
            let first_server = &servers[0];
            info!("Attempting to connect to game server: {} (game_id: {}, uri: {})", first_server.name, first_server.game_id, first_server.uri);

            // Parse game server URI and connect
            if let Err(e) = connect_to_game_server(&first_server.uri, &username, &password, &first_server.game_id, &locale, &app_paths).await {
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
