//! ECS API for Scripting Runtimes
//!
//! This module provides ECS (Entity-Component-System) primitives that can be used
//! by mods to create entities, add/remove components, and query the world.
//!
//! # Architecture
//!
//! The ECS API follows a dual-layer approach:
//!
//! 1. **Script Components**: Custom components defined by mods, stored as JSON data
//! 2. **Native Components**: Bevy's built-in components (Transform, Sprite, etc.)
//!    accessed via reflection
//!
//! All operations are thread-safe and go through the command channel to the
//! Bevy main thread.

mod types;

pub use types::*;
