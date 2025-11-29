//! Terminal Input Handler
//!
//! Provides cross-platform terminal input handling with raw mode support.
//! This module allows intercepting all keyboard events before they are
//! processed by the default terminal handler.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal;
use crossterm::execute;
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::api::TerminalKeyRequest;

/// Global flag indicating whether terminal is in raw mode
/// Used by logging to determine if \r\n should be used instead of \n
static RAW_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Check if the terminal is currently in raw mode
///
/// This is used by logging systems to determine if they need to use
/// `\r\n` line endings instead of just `\n`.
pub fn is_raw_mode_active() -> bool {
    RAW_MODE_ACTIVE.load(Ordering::Relaxed)
}

/// Terminal input handler that reads keyboard events in raw mode
pub struct TerminalInputHandler {
    /// Whether raw mode is currently enabled
    raw_mode_enabled: bool,
}

impl TerminalInputHandler {
    /// Create a new terminal input handler
    pub fn new() -> Self {
        Self {
            raw_mode_enabled: false,
        }
    }

    /// Enable raw mode on the terminal
    ///
    /// In raw mode, keyboard input is not echoed and is available immediately
    /// without waiting for Enter. This allows intercepting all key presses.
    pub fn enable_raw_mode(&mut self) -> io::Result<()> {
        if !self.raw_mode_enabled {
            terminal::enable_raw_mode()?;
            self.raw_mode_enabled = true;
            debug!("Terminal raw mode enabled");
        }
        Ok(())
    }

    /// Disable raw mode on the terminal
    ///
    /// This restores normal terminal behavior where input is line-buffered
    /// and echoed to the screen.
    pub fn disable_raw_mode(&mut self) -> io::Result<()> {
        if self.raw_mode_enabled {
            terminal::disable_raw_mode()?;
            self.raw_mode_enabled = false;
            debug!("Terminal raw mode disabled");
        }
        Ok(())
    }

    /// Check if raw mode is currently enabled
    pub fn is_raw_mode_enabled(&self) -> bool {
        self.raw_mode_enabled
    }
}

impl Default for TerminalInputHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TerminalInputHandler {
    fn drop(&mut self) {
        // Ensure raw mode is disabled when the handler is dropped
        if self.raw_mode_enabled {
            if let Err(e) = terminal::disable_raw_mode() {
                error!("Failed to disable raw mode on drop: {}", e);
            }
        }
    }
}

/// Convert a crossterm KeyEvent to a TerminalKeyRequest
pub fn key_event_to_request(key_event: &KeyEvent) -> TerminalKeyRequest {
    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key_event.modifiers.contains(KeyModifiers::ALT);
    let meta = key_event.modifiers.contains(KeyModifiers::META)
        || key_event.modifiers.contains(KeyModifiers::SUPER);

    // For character keys, we need to handle shift specially:
    // - If the character is uppercase, shift is implied even if not reported
    // - We normalize the key to lowercase for consistency
    let (key, shift) = match &key_event.code {
        KeyCode::Char(c) => {
            let is_uppercase = c.is_uppercase();
            let explicit_shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
            // Shift is true if explicitly pressed OR if character is uppercase
            let shift = explicit_shift || is_uppercase;
            // Normalize to lowercase for consistent key matching
            (c.to_lowercase().to_string(), shift)
        }
        _ => {
            let shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
            (key_code_to_string(&key_event.code), shift)
        }
    };

    TerminalKeyRequest::new(key, ctrl, alt, shift, meta)
}

/// Convert a KeyCode to a string representation
fn key_code_to_string(code: &KeyCode) -> String {
    match code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{}", n),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Escape".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Up => "ArrowUp".to_string(),
        KeyCode::Down => "ArrowDown".to_string(),
        KeyCode::Left => "ArrowLeft".to_string(),
        KeyCode::Right => "ArrowRight".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::CapsLock => "CapsLock".to_string(),
        KeyCode::ScrollLock => "ScrollLock".to_string(),
        KeyCode::NumLock => "NumLock".to_string(),
        KeyCode::PrintScreen => "PrintScreen".to_string(),
        KeyCode::Pause => "Pause".to_string(),
        KeyCode::Menu => "Menu".to_string(),
        KeyCode::KeypadBegin => "KeypadBegin".to_string(),
        KeyCode::Null => "Null".to_string(),
        _ => "Unknown".to_string(),
    }
}

/// Handle for the terminal event reader thread
///
/// When dropped, this will signal the thread to stop and wait for cleanup to complete.
pub struct TerminalReaderHandle {
    cancel_tx: Option<mpsc::Sender<()>>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl TerminalReaderHandle {
    /// Stop the terminal reader and wait for cleanup to complete
    pub fn stop(&mut self) {
        // Send cancel signal
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.blocking_send(());
        }
        // Wait for thread to complete cleanup
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }

    /// Stop the terminal reader asynchronously
    pub async fn stop_async(&mut self) {
        // Send cancel signal
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(()).await;
        }
        // Wait for thread to complete cleanup in a blocking task
        if let Some(handle) = self.join_handle.take() {
            tokio::task::spawn_blocking(move || {
                let _ = handle.join();
            }).await.ok();
        }
    }
}

impl Drop for TerminalReaderHandle {
    fn drop(&mut self) {
        // Ensure cleanup happens even if stop() wasn't called explicitly
        self.stop();
    }
}

/// Spawn a task that reads terminal events and sends them to a channel
///
/// This function enables raw mode and starts reading keyboard events.
/// Events are sent to the returned receiver channel.
///
/// # Returns
/// A tuple of (receiver, handle) where:
/// - receiver: Receives TerminalKeyRequest for each key press
/// - handle: Handle to stop the reader and ensure cleanup. When dropped, it will
///           automatically stop the reader and wait for the terminal to be restored.
pub fn spawn_terminal_event_reader() -> io::Result<(
    mpsc::Receiver<TerminalKeyRequest>,
    TerminalReaderHandle,
)> {
    // Enable raw mode
    terminal::enable_raw_mode()?;
    RAW_MODE_ACTIVE.store(true, Ordering::Relaxed);

    // Try to enable keyboard enhancement for better modifier key detection
    // This uses the kitty keyboard protocol which is supported by modern terminals
    // (kitty, foot, WezTerm, alacritty, etc.)
    let keyboard_enhancement_enabled = if terminal::supports_keyboard_enhancement().unwrap_or(false) {
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        if execute!(std::io::stdout(), PushKeyboardEnhancementFlags(flags)).is_ok() {
            debug!("Keyboard enhancement enabled (kitty protocol)");
            true
        } else {
            debug!("Failed to enable keyboard enhancement");
            false
        }
    } else {
        debug!("Terminal does not support keyboard enhancement (Ctrl+Shift combinations may not be detected)");
        false
    };

    let (event_tx, event_rx) = mpsc::channel::<TerminalKeyRequest>(32);
    let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

    // Spawn blocking task to read terminal events
    let join_handle = std::thread::spawn(move || {
        loop {
            // Check for cancellation
            if cancel_rx.try_recv().is_ok() {
                debug!("Terminal event reader cancelled");
                break;
            }

            // Poll for events with a timeout
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => {
                    match event::read() {
                        Ok(Event::Key(key_event)) => {
                            let request = key_event_to_request(&key_event);
                            debug!("Key event: {:?}", request.combo);

                            // Send to channel (blocking send since we're in a thread)
                            if event_tx.blocking_send(request).is_err() {
                                // Channel closed, exit
                                break;
                            }
                        }
                        Ok(_) => {
                            // Ignore non-key events (mouse, resize, etc.)
                        }
                        Err(e) => {
                            error!("Error reading terminal event: {}", e);
                            break;
                        }
                    }
                }
                Ok(false) => {
                    // No event available, continue polling
                }
                Err(e) => {
                    error!("Error polling terminal events: {}", e);
                    break;
                }
            }
        }

        // Disable keyboard enhancement if it was enabled
        if keyboard_enhancement_enabled {
            let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
        }

        // Disable raw mode when done
        RAW_MODE_ACTIVE.store(false, Ordering::Relaxed);
        if let Err(e) = terminal::disable_raw_mode() {
            error!("Failed to disable raw mode: {}", e);
        }
        debug!("Terminal event reader stopped");
    });

    let handle = TerminalReaderHandle {
        cancel_tx: Some(cancel_tx),
        join_handle: Some(join_handle),
    };

    Ok((event_rx, handle))
}

/// RAII guard that enables raw mode on creation and disables it on drop
pub struct RawModeGuard {
    enabled: bool,
}

impl RawModeGuard {
    /// Enable raw mode and return a guard that will disable it on drop
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        debug!("Terminal raw mode enabled (guard)");
        Ok(Self { enabled: true })
    }

    /// Check if raw mode is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.enabled {
            if let Err(e) = terminal::disable_raw_mode() {
                error!("Failed to disable raw mode (guard): {}", e);
            } else {
                debug!("Terminal raw mode disabled (guard)");
            }
        }
    }
}

/// Check if stdin is connected to a terminal (TTY)
pub fn is_terminal() -> bool {
    atty::is(atty::Stream::Stdin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_code_to_string() {
        assert_eq!(key_code_to_string(&KeyCode::Char('a')), "a");
        assert_eq!(key_code_to_string(&KeyCode::Char('Z')), "Z");
        assert_eq!(key_code_to_string(&KeyCode::F(1)), "F1");
        assert_eq!(key_code_to_string(&KeyCode::F(12)), "F12");
        assert_eq!(key_code_to_string(&KeyCode::Enter), "Enter");
        assert_eq!(key_code_to_string(&KeyCode::Esc), "Escape");
        assert_eq!(key_code_to_string(&KeyCode::Up), "ArrowUp");
    }

    #[test]
    fn test_key_event_to_request() {
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let request = key_event_to_request(&event);
        assert_eq!(request.key, "c");
        assert!(request.ctrl);
        assert!(!request.alt);
        assert!(!request.shift);
        assert!(!request.meta);
        assert_eq!(request.combo, "Ctrl+c");
    }

    #[test]
    fn test_key_event_to_request_multiple_modifiers() {
        let event = KeyEvent::new(
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let request = key_event_to_request(&event);
        assert_eq!(request.key, "z");
        assert!(request.ctrl);
        assert!(!request.alt);
        assert!(request.shift);
        assert_eq!(request.combo, "Ctrl+Shift+z");
    }

    #[test]
    fn test_key_event_uppercase_implies_shift() {
        // When terminal sends uppercase char (e.g., Ctrl+Shift+C sends 'C'),
        // we should detect shift from the uppercase and normalize to lowercase
        let event = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::CONTROL);
        let request = key_event_to_request(&event);
        assert_eq!(request.key, "c"); // Normalized to lowercase
        assert!(request.ctrl);
        assert!(!request.alt);
        assert!(request.shift); // Shift inferred from uppercase
        assert!(!request.meta);
        assert_eq!(request.combo, "Ctrl+Shift+c");
    }

    #[test]
    fn test_key_event_lowercase_no_shift() {
        let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let request = key_event_to_request(&event);
        assert_eq!(request.key, "a");
        assert!(!request.ctrl);
        assert!(!request.alt);
        assert!(!request.shift);
        assert!(!request.meta);
        assert_eq!(request.combo, "a");
    }
}
