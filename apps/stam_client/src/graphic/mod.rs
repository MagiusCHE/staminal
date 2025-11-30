//! Graphic Engine Implementations for Staminal Client
//!
//! This module contains client-specific graphic engine implementations.
//! Currently supports Bevy as the primary engine.

mod bevy_engine;

pub use bevy_engine::BevyEngine;
