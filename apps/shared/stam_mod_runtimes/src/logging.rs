//! Shared logging utilities for Staminal
//!
//! Provides a custom formatter for tracing that:
//! - Formats thread IDs as #N instead of ThreadId(N)
//! - Extracts `runtime_type` and `mod_id` fields to display as `js::mod-id`
//! - Strips common prefixes from targets for cleaner output
//! - Handles raw mode terminal output with proper `\r\n` line endings
//!
//! # Usage
//!
//! ```rust,ignore
//! use stam_mod_runtimes::logging::{CustomFormatter, init_logging};
//! use time::UtcOffset;
//!
//! // Simple initialization with defaults
//! init_logging(true, None, None)?;
//!
//! // Or with file logging
//! let file = std::fs::File::create("app.log")?;
//! init_logging(true, Some(file), None)?;
//! ```

use std::fmt as std_fmt;
use std::io::{self, Write};
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

use crate::terminal_input;

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
        // Only convert if raw mode is active
        #[cfg(unix)]
        let raw_mode = terminal_input::is_raw_mode_active();
        #[cfg(not(unix))]
        let raw_mode = false;

        if raw_mode {
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

/// Initialize logging with a custom formatter
///
/// # Arguments
/// * `use_ansi` - Whether to use ANSI color codes (typically based on TTY detection)
/// * `log_file` - Optional file to write logs to (in addition to stdout)
/// * `strip_prefix` - Optional prefix to strip from log targets (e.g., "stam_server::")
/// * `level` - The minimum log level to output
///
/// # Example
///
/// ```rust,ignore
/// use stam_mod_runtimes::logging::init_logging;
/// use tracing::Level;
///
/// init_logging(true, None, Some("stam_server::"), Level::DEBUG)?;
/// ```
pub fn init_logging<W: Write + Send + 'static>(
    use_ansi: bool,
    log_file: Option<W>,
    strip_prefix: Option<&str>,
    level: Level,
) -> Result<(), Box<dyn std::error::Error>> {
    let timer = create_custom_timer();

    if let Some(file) = log_file {
        let formatter_stdout = CustomFormatter::new(timer.clone(), use_ansi);
        let formatter_stdout = if let Some(prefix) = strip_prefix {
            formatter_stdout.with_strip_prefix(prefix)
        } else {
            formatter_stdout
        };

        let formatter_file = CustomFormatter::new(timer, false);
        let formatter_file = if let Some(prefix) = strip_prefix {
            formatter_file.with_strip_prefix(prefix)
        } else {
            formatter_file
        };

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_stdout)
                    .with_writer(std::io::stdout),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter_file)
                    .with_writer(std::sync::Mutex::new(file)),
            )
            .with(tracing_subscriber::filter::LevelFilter::from_level(level))
            .init();
    } else {
        let formatter = CustomFormatter::new(timer, use_ansi);
        let formatter = if let Some(prefix) = strip_prefix {
            formatter.with_strip_prefix(prefix)
        } else {
            formatter
        };

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(formatter)
                    .with_writer(std::io::stdout),
            )
            .with(tracing_subscriber::filter::LevelFilter::from_level(level))
            .init();
    }

    Ok(())
}
