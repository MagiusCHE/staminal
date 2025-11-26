//! System API for Mod Runtimes
//!
//! Provides access to mod information and system state.
//! The `system.get_mods()` function returns an array of mod info objects.

use std::sync::{Arc, RwLock};

/// Information about a mod
///
/// This struct is returned by `system.get_mods()` to provide
/// mods with visibility into what other mods are available.
#[derive(Clone, Debug)]
pub struct ModInfo {
    /// Unique identifier for the mod (directory name)
    pub id: String,
    /// Mod version from manifest
    pub version: String,
    /// Human-readable name from manifest
    pub name: String,
    /// Description from manifest
    pub description: String,
    /// Mod type: "bootstrap" or "library"
    pub mod_type: Option<String>,
    /// Load priority (lower numbers load first)
    pub priority: i32,
    /// Whether onBootstrap has been called for this mod
    pub bootstrapped: bool,
    /// Whether this mod has been loaded into the runtime
    pub loaded: bool,
}

/// System API providing access to mod registry and system state
///
/// This API is shared across all mod contexts and provides read-only
/// access to the list of loaded mods. The list is created once when
/// mods are loaded, and each call to `get_mods()` returns a fresh copy.
#[derive(Clone)]
pub struct SystemApi {
    /// Shared registry of all loaded mods
    mods: Arc<RwLock<Vec<ModInfo>>>,
}

impl SystemApi {
    /// Create a new SystemApi with an empty mod list
    pub fn new() -> Self {
        Self {
            mods: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a mod to the registry
    ///
    /// This should be called when a mod is loaded into the runtime.
    pub fn register_mod(&self, mod_info: ModInfo) {
        let mut mods = self.mods.write().unwrap();
        // Check if mod already exists (by id) and update it
        if let Some(existing) = mods.iter_mut().find(|m| m.id == mod_info.id) {
            *existing = mod_info;
        } else {
            mods.push(mod_info);
        }
    }

    /// Mark a mod as bootstrapped
    ///
    /// This should be called after onBootstrap is invoked for a mod.
    pub fn set_bootstrapped(&self, mod_id: &str, bootstrapped: bool) {
        let mut mods = self.mods.write().unwrap();
        if let Some(mod_info) = mods.iter_mut().find(|m| m.id == mod_id) {
            mod_info.bootstrapped = bootstrapped;
        }
    }

    /// Mark a mod as loaded
    ///
    /// This should be called after a mod is loaded into the runtime.
    pub fn set_loaded(&self, mod_id: &str, loaded: bool) {
        let mut mods = self.mods.write().unwrap();
        if let Some(mod_info) = mods.iter_mut().find(|m| m.id == mod_id) {
            mod_info.loaded = loaded;
        }
    }

    /// Get a copy of all registered mods
    ///
    /// Returns mods sorted by: loaded mods first (sorted by priority), then not-yet-loaded mods.
    /// Each call returns a fresh copy of the mod list.
    pub fn get_mods(&self) -> Vec<ModInfo> {
        let mods = self.mods.read().unwrap();

        // Separate loaded and not-yet-loaded mods
        let mut loaded: Vec<ModInfo> = mods.iter().filter(|m| m.loaded).cloned().collect();
        let mut not_loaded: Vec<ModInfo> = mods.iter().filter(|m| !m.loaded).cloned().collect();

        // Sort loaded mods by priority (lower first)
        loaded.sort_by_key(|m| m.priority);
        // Sort not-loaded mods by priority as well
        not_loaded.sort_by_key(|m| m.priority);

        // Concatenate: loaded first, then not-loaded
        loaded.extend(not_loaded);
        loaded
    }

    /// Get information about a specific mod by ID
    pub fn get_mod(&self, mod_id: &str) -> Option<ModInfo> {
        let mods = self.mods.read().unwrap();
        mods.iter().find(|m| m.id == mod_id).cloned()
    }

    /// Get the number of registered mods
    pub fn mod_count(&self) -> usize {
        let mods = self.mods.read().unwrap();
        mods.len()
    }
}

impl Default for SystemApi {
    fn default() -> Self {
        Self::new()
    }
}
