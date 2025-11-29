//! Bevy game engine integration for Staminal Client
//!
//! This module provides the game window and UI rendering using Bevy + bevy_egui.
//! It communicates with the mod runtimes via crossbeam channels.

pub mod app;
pub mod ui_bridge;
pub mod systems;

pub use app::{StaminalApp, run_bevy_app};
pub use ui_bridge::{UiBridge, ShutdownHandle, ShutdownReceiver};
