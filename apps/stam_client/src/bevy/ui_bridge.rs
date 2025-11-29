//! UI Bridge - Communication between Bevy and mod runtimes
//!
//! This module provides thread-safe channels for UI commands and events
//! between the Bevy render thread and the mod runtime system.

use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::watch;

use stam_mod_runtimes::api::{
    UiApi, UiCommand, UiEvent, WindowApi, WindowCommand,
};

/// Shutdown handle to signal shutdown from any thread
#[derive(Clone)]
pub struct ShutdownHandle {
    shutdown_flag: Arc<AtomicBool>,
    shutdown_tx: watch::Sender<bool>,
}

impl ShutdownHandle {
    /// Signal shutdown
    pub fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(true);
    }
}

/// Receiver for shutdown notifications (can be awaited)
#[derive(Clone)]
pub struct ShutdownReceiver {
    shutdown_rx: watch::Receiver<bool>,
}

impl ShutdownReceiver {
    /// Wait for shutdown signal (async)
    pub async fn wait(&mut self) {
        // Wait until the value becomes true
        while !*self.shutdown_rx.borrow() {
            if self.shutdown_rx.changed().await.is_err() {
                break; // Sender dropped
            }
        }
    }
}

/// Bridge for communication between Bevy and mod runtimes
///
/// This struct holds the channel endpoints and shared state for:
/// - UI commands from scripts -> Bevy renderer
/// - UI events from Bevy -> scripts
/// - Window commands from scripts -> Bevy
/// - Window state shared between Bevy and scripts
#[derive(Clone, Resource)]
pub struct UiBridge {
    /// Channel to receive UI commands from scripts
    pub ui_command_rx: Receiver<UiCommand>,
    /// Channel to send UI events to scripts
    pub ui_event_tx: Sender<UiEvent>,
    /// Channel to receive window commands from scripts
    pub window_command_rx: Receiver<WindowCommand>,
    /// Shared window size (updated by Bevy, read by scripts)
    pub window_size: Arc<RwLock<(u32, u32)>>,
    /// Shutdown flag (set by client thread to signal Bevy to exit)
    pub shutdown_flag: Arc<AtomicBool>,
}

impl UiBridge {
    /// Create a new UiBridge with connected channels
    ///
    /// Returns the bridge (for Bevy), the APIs (for mod runtimes), shutdown handle, and shutdown receiver
    pub fn new() -> (Self, UiApi, WindowApi, ShutdownHandle, ShutdownReceiver) {
        // Create UI channels
        let (ui_command_tx, ui_command_rx) = unbounded::<UiCommand>();
        let (ui_event_tx, ui_event_rx) = unbounded::<UiEvent>();

        // Create Window channels
        let (window_command_tx, window_command_rx) = unbounded::<WindowCommand>();
        let window_size = Arc::new(RwLock::new((1280u32, 720u32)));

        // Create shutdown flag and watch channel
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Create APIs for mod runtimes
        let ui_api = UiApi::new(ui_command_tx, ui_event_rx);
        let window_api = WindowApi::new(window_command_tx, window_size.clone());

        // Create shutdown handle and receiver
        let shutdown_handle = ShutdownHandle {
            shutdown_flag: shutdown_flag.clone(),
            shutdown_tx,
        };
        let shutdown_receiver = ShutdownReceiver { shutdown_rx };

        let bridge = Self {
            ui_command_rx,
            ui_event_tx,
            window_command_rx,
            window_size,
            shutdown_flag,
        };

        (bridge, ui_api, window_api, shutdown_handle, shutdown_receiver)
    }

    /// Check if shutdown has been requested
    pub fn should_shutdown(&self) -> bool {
        self.shutdown_flag.load(Ordering::SeqCst)
    }

    /// Update the cached window size (called by Bevy when window is resized)
    pub fn update_window_size(&self, width: u32, height: u32) {
        if let Ok(mut size) = self.window_size.write() {
            *size = (width, height);
        }
    }

    /// Poll for pending UI commands (non-blocking)
    pub fn poll_ui_commands(&self) -> Vec<UiCommand> {
        let mut commands = Vec::new();
        while let Ok(cmd) = self.ui_command_rx.try_recv() {
            commands.push(cmd);
        }
        commands
    }

    /// Poll for pending window commands (non-blocking)
    pub fn poll_window_commands(&self) -> Vec<WindowCommand> {
        let mut commands = Vec::new();
        while let Ok(cmd) = self.window_command_rx.try_recv() {
            commands.push(cmd);
        }
        commands
    }

    /// Send a UI event to scripts
    pub fn send_ui_event(&self, event: UiEvent) {
        let _ = self.ui_event_tx.send(event);
    }
}

impl Default for UiBridge {
    fn default() -> Self {
        Self::new().0
    }
}
