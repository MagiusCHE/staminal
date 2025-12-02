//! Centralized logging for Staminal applications
//!
//! Provides a custom formatter for tracing that:
//! - Formats thread IDs as #N instead of ThreadId(N)
//! - Extracts `runtime_type` and `mod_id` fields to display as `js::mod-id`
//! - Strips common prefixes from targets for cleaner output
//! - Handles raw mode terminal output with proper `\r\n` line endings
//! - Filters external dependency logs based on `STAM_LOGDEPS` environment variable
//!
//! # Environment Variables
//!
//! - `STAM_LOGDEPS`: Set to `1` to enable logging from external dependencies (bevy, wgpu, etc.).
//!   Default is `0` which only shows logs from Staminal code.
//!
//! # Usage
//!
//! ```rust,ignore
//! use stam_log::{init_logging, LogConfig};
//! use tracing::Level;
//!
//! // Simple initialization with defaults
//! let config = LogConfig::new("stam_client::");
//! init_logging(config)?;
//!
//! // Or with file logging
//! let file = std::fs::File::create("app.log")?;
//! let config = LogConfig::new("stam_server::")
//!     .with_log_file(file)
//!     .with_level(Level::DEBUG);
//! init_logging(config)?;
//! ```

use std::fmt as std_fmt;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::Level;
use tracing::field::Field;
use tracing_subscriber::field::Visit;
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::fmt::{
    self, FmtContext, FormatEvent, FormatFields, MakeWriter, format::Writer,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

/// Global flag indicating whether terminal is in raw mode
/// Used by logging to determine if \r\n should be used instead of \n
static RAW_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set the raw mode state for logging
///
/// When raw mode is active, the logger will use `\r\n` line endings
/// instead of just `\n` for proper terminal output.
pub fn set_raw_mode_active(active: bool) {
    RAW_MODE_ACTIVE.store(active, Ordering::Relaxed);
}

/// Check if the terminal is currently in raw mode
///
/// This is used by logging systems to determine if they need to use
/// `\r\n` line endings instead of just `\n`.
pub fn is_raw_mode_active() -> bool {
    RAW_MODE_ACTIVE.load(Ordering::Relaxed)
}

/// A writer that converts `\n` to `\r\n` for raw mode terminal output.
///
/// In raw mode, the terminal doesn't automatically convert newlines,
/// so we need to explicitly use carriage return + line feed.
pub struct RawModeWriter<W> {
    inner: W,
}

impl<W: Write> RawModeWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> Write for RawModeWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if is_raw_mode_active() {
            // Convert \n to \r\n
            let mut start = 0;
            for (i, &byte) in buf.iter().enumerate() {
                if byte == b'\n' {
                    // Write everything before the \n
                    if i > start {
                        self.inner.write_all(&buf[start..i])?;
                    }
                    // Write \r\n instead of just \n
                    self.inner.write_all(b"\r\n")?;
                    start = i + 1;
                }
            }
            // Write remaining bytes
            if start < buf.len() {
                self.inner.write_all(&buf[start..])?;
            }
            Ok(buf.len())
        } else {
            self.inner.write(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// A MakeWriter that wraps stdout with RawModeWriter
pub struct RawModeStdoutWriter;

impl<'a> MakeWriter<'a> for RawModeStdoutWriter {
    type Writer = RawModeWriter<io::Stdout>;

    fn make_writer(&'a self) -> Self::Writer {
        RawModeWriter::new(io::stdout())
    }
}

/// Field extractor for game_id, runtime_type, mod_id, and message fields
///
/// Used by the custom formatter to detect mod-related log messages
/// and format them as `game_id::js::mod-id: message` or `js::mod-id: message`.
#[derive(Default)]
pub struct FieldExtractor {
    pub game_id: Option<String>,
    pub runtime_type: Option<String>,
    pub mod_id: Option<String>,
    pub message: Option<String>,
}

impl Visit for FieldExtractor {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "game_id" => self.game_id = Some(value.to_string()),
            "runtime_type" => self.runtime_type = Some(value.to_string()),
            "mod_id" => self.mod_id = Some(value.to_string()),
            "message" => self.message = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std_fmt::Debug) {
        match field.name() {
            "game_id" => self.game_id = Some(format!("{:?}", value).trim_matches('"').to_string()),
            "runtime_type" => {
                self.runtime_type = Some(format!("{:?}", value).trim_matches('"').to_string())
            }
            "mod_id" => self.mod_id = Some(format!("{:?}", value).trim_matches('"').to_string()),
            "message" => self.message = Some(format!("{:?}", value).trim_matches('"').to_string()),
            _ => {}
        }
    }
}

/// Custom event formatter for Staminal applications
///
/// Features:
/// - Thread IDs displayed as #N instead of ThreadId(N)
/// - Mod logs formatted as `js::mod-id: message`
/// - Configurable ANSI color support
/// - Configurable target prefix stripping
pub struct CustomFormatter<T> {
    timer: T,
    ansi: bool,
    /// Prefix to strip from log targets (e.g., "stam_server::" or "stam_client::")
    strip_prefix: Option<String>,
}

impl<T> CustomFormatter<T> {
    /// Create a new CustomFormatter
    ///
    /// # Arguments
    /// * `timer` - The time formatter to use
    /// * `ansi` - Whether to use ANSI color codes
    pub fn new(timer: T, ansi: bool) -> Self {
        Self {
            timer,
            ansi,
            strip_prefix: None,
        }
    }

    /// Set the prefix to strip from log targets
    ///
    /// # Arguments
    /// * `prefix` - The prefix to strip (e.g., "stam_server::")
    pub fn with_strip_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.strip_prefix = Some(prefix.into());
        self
    }
}

impl<T: Clone> Clone for CustomFormatter<T> {
    fn clone(&self) -> Self {
        Self {
            timer: self.timer.clone(),
            ansi: self.ansi,
            strip_prefix: self.strip_prefix.clone(),
        }
    }
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

        let (dim_start, dim_end) = if self.ansi {
            ("\x1b[2m", "\x1b[0m")
        } else {
            ("", "")
        };
        let (level_color, level_str) = match *metadata.level() {
            Level::ERROR => (if self.ansi { "\x1b[31m" } else { "" }, "ERROR"),
            Level::WARN => (if self.ansi { "\x1b[33m" } else { "" }, " WARN"),
            Level::INFO => (if self.ansi { "\x1b[32m" } else { "" }, " INFO"),
            Level::DEBUG => (if self.ansi { "\x1b[34m" } else { "" }, "DEBUG"),
            Level::TRACE => (if self.ansi { "\x1b[35m" } else { "" }, "TRACE"),
        };
        let color_end = if self.ansi { "\x1b[0m" } else { "" };

        write!(writer, "{}", dim_start)?;
        self.timer.format_time(&mut writer)?;
        write!(writer, "{} ", dim_end)?;

        write!(writer, "{}{}{} ", level_color, level_str, color_end)?;

        let thread_id = format!("{:?}", std::thread::current().id());
        if let Some(num_str) = thread_id
            .strip_prefix("ThreadId(")
            .and_then(|s| s.strip_suffix(")"))
        {
            if let Ok(num) = num_str.parse::<u64>() {
                write!(writer, "#{:03} ", num)?;
            }
        }

        // Extract game_id, runtime_type and mod_id fields if present
        let mut extractor = FieldExtractor::default();
        event.record(&mut extractor);

        // If both runtime_type and mod_id are present, format as "game_id::runtime_type::mod_id:" or "runtime_type::mod_id:"
        if let (Some(rt), Some(mid)) = (&extractor.runtime_type, &extractor.mod_id) {
            if let Some(gid) = &extractor.game_id {
                write!(writer, "{}{}::{}::{}{}: ", dim_start, gid, rt, mid, dim_end)?;
            } else {
                write!(writer, "{}{}::{}{}: ", dim_start, rt, mid, dim_end)?;
            }
            // Print the message if present
            if let Some(msg) = &extractor.message {
                write!(writer, "{}", msg)?;
            }
        } else {
            // Otherwise use the default target formatting
            let target = metadata.target();

            // Check if this target belongs to our app (starts with our prefix)
            let is_our_code = self.strip_prefix.as_ref()
                .is_some_and(|prefix| target.starts_with(prefix.trim_end_matches("::")));

            let display_target = if is_our_code {
                // Strip our prefix for cleaner output
                if let Some(prefix) = &self.strip_prefix {
                    target.strip_prefix(prefix).unwrap_or(target)
                } else {
                    target
                }
            } else {
                // External dependency - show full target
                target
            };

            // Also hide the bare app name when it appears alone
            let app_name = self.strip_prefix.as_ref().map(|p| p.trim_end_matches("::"));
            let should_hide = app_name.is_some_and(|name| display_target == name);

            if !display_target.is_empty() && !should_hide {
                write!(writer, "{}{}{}: ", dim_start, display_target, dim_end)?;
            }
            // Use default field formatting
            ctx.field_format().format_fields(writer.by_ref(), event)?;
        }

        writeln!(writer)
    }
}

/// Create a default timer with local UTC offset
///
/// Falls back to UTC if local offset cannot be determined.
pub fn create_default_timer() -> OffsetTime<time::format_description::well_known::Rfc3339> {
    use time::format_description::well_known::Rfc3339;

    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    OffsetTime::new(offset, Rfc3339)
}

/// Create a timer with custom format
///
/// Uses format: `[year]/[month]/[day] [hour]:[minute]:[second].[subsecond digits:4]`
pub fn create_custom_timer()
-> OffsetTime<&'static [time::format_description::BorrowedFormatItem<'static>]> {
    use time::macros::format_description;

    let format =
        format_description!("[year]/[month]/[day] [hour]:[minute]:[second].[subsecond digits:4]");
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    OffsetTime::new(offset, format)
}

/// Check if dependency logging is enabled via STAM_LOGDEPS environment variable
///
/// Returns `true` if `STAM_LOGDEPS=1`, `false` otherwise (default).
pub fn is_dependency_logging_enabled() -> bool {
    std::env::var("STAM_LOGDEPS")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Build the filter directive for dependency logging
///
/// When `STAM_LOGDEPS=0` (default), only logs from Staminal code are shown.
/// When `STAM_LOGDEPS=1`, all logs are shown including external dependencies.
pub fn build_filter_directives(level: Level, log_deps: bool) -> String {
    let level_str = match level {
        Level::TRACE => "trace",
        Level::DEBUG => "debug",
        Level::INFO => "info",
        Level::WARN => "warn",
        Level::ERROR => "error",
    };

    if log_deps {
        // Show all logs at the specified level
        level_str.to_string()
    } else {
        // Only show logs from Staminal code (stam_*) at the specified level
        // External dependencies are filtered to OFF to reduce noise completely
        format!(
            "off,stam_client={level},stam_server={level},stam_protocol={level},stam_schema={level},stam_mod_runtimes={level},stam_log={level},js={level}",
            level = level_str
        )
    }
}

/// Detect if ANSI colors should be used based on environment
///
/// Disables ANSI colors if:
/// - stdout is not a TTY (piped/redirected)
/// - NO_COLOR env var is set (https://no-color.org/)
/// - TERM=dumb
pub fn should_use_ansi() -> bool {
    atty::is(atty::Stream::Stdout)
        && std::env::var("NO_COLOR").is_err()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
}

/// Logging configuration
pub struct LogConfig<W: Write + Send + 'static = std::fs::File> {
    /// Prefix to strip from log targets (e.g., "stam_client::")
    pub strip_prefix: String,
    /// Whether to use ANSI color codes (auto-detected if None)
    pub use_ansi: Option<bool>,
    /// Minimum log level
    pub level: Level,
    /// Optional file to write logs to
    pub log_file: Option<W>,
}

impl<W: Write + Send + 'static> LogConfig<W> {
    /// Create a new LogConfig with the given strip prefix
    pub fn new(strip_prefix: impl Into<String>) -> Self {
        Self {
            strip_prefix: strip_prefix.into(),
            use_ansi: None,
            level: Level::DEBUG,
            log_file: None,
        }
    }

    /// Set whether to use ANSI colors (default: auto-detect)
    pub fn with_ansi(mut self, use_ansi: bool) -> Self {
        self.use_ansi = Some(use_ansi);
        self
    }

    /// Set the minimum log level
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Set the log file
    pub fn with_log_file(mut self, file: W) -> Self {
        self.log_file = Some(file);
        self
    }
}

/// Initialize logging with the given configuration
///
/// # Arguments
/// * `config` - Logging configuration
///
/// # Environment Variables
///
/// * `STAM_LOGDEPS` - Set to `1` to enable logging from external dependencies (bevy, wgpu, etc.).
///   Default is `0` which only shows logs from Staminal code.
/// * `RUST_LOG` - Can override the default filter directives
///
/// # Example
///
/// ```rust,ignore
/// use stam_log::{init_logging, LogConfig};
/// use tracing::Level;
///
/// let config = LogConfig::new("stam_client::")
///     .with_level(Level::DEBUG);
/// init_logging(config)?;
/// ```
pub fn init_logging<W: Write + Send + 'static>(
    config: LogConfig<W>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::EnvFilter;

    let timer = create_custom_timer();
    let use_ansi = config.use_ansi.unwrap_or_else(should_use_ansi);
    let log_deps = is_dependency_logging_enabled();
    let filter_directives = build_filter_directives(config.level, log_deps);

    // Create the env filter - allows RUST_LOG to override our defaults
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&filter_directives));

    if let Some(file) = config.log_file {
        let formatter_stdout = CustomFormatter::new(timer.clone(), use_ansi)
            .with_strip_prefix(&config.strip_prefix);
        let formatter_file = CustomFormatter::new(timer, false)
            .with_strip_prefix(&config.strip_prefix);

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_stdout)
                    .with_ansi(use_ansi)
                    .with_writer(RawModeStdoutWriter),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_file)
                    .with_ansi(false)
                    .with_writer(std::sync::Mutex::new(file)),
            )
            .with(env_filter)
            .init();
    } else {
        let formatter = CustomFormatter::new(timer, use_ansi)
            .with_strip_prefix(&config.strip_prefix);

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter)
                    .with_ansi(use_ansi)
                    .with_writer(RawModeStdoutWriter),
            )
            .with(env_filter)
            .init();
    }

    Ok(())
}

/// Initialize logging without a log file
///
/// This is a convenience function for simpler cases.
pub fn init_logging_simple(
    strip_prefix: impl Into<String>,
    level: Level,
) -> Result<(), Box<dyn std::error::Error>> {
    let config: LogConfig<std::fs::File> = LogConfig::new(strip_prefix)
        .with_level(level);
    init_logging(config)
}
