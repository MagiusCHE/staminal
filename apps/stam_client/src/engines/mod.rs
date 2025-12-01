//! Graphic Engine Implementations
//!
//! This module contains the implementations of graphic engines for the Staminal client.
//! Currently, only Bevy is implemented, but the architecture supports future engines
//! like raw WGPU or terminal-based rendering.

pub mod bevy;

pub use bevy::BevyEngine;
