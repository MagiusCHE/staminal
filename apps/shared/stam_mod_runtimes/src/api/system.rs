//! System API for Mod Runtimes
//!
//! Provides access to mod information and system state.
//! The `system.get_mods()` function returns an array of mod info objects.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use super::events::EventDispatcher;

/// Filter for mod packages (client or server side)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ModSide {
    Client = 0,
    Server = 1,
}

impl ModSide {
    /// Convert from u32 (for JavaScript interop)
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(ModSide::Client),
            1 => Some(ModSide::Server),
            _ => None,
        }
    }

    /// Convert to u32 (for JavaScript interop)
    pub fn to_u32(self) -> u32 {
        self as u32
    }
}

/// Manifest information from mod-packages.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModPackageManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub entry_point: String,
    #[serde(default)]
    pub execute_on: serde_json::Value, // Can be string or array
    #[serde(default)]
    pub priority: i32,
    #[serde(rename = "type", default)]
    pub mod_type: Option<String>,
    #[serde(default)]
    pub requires: HashMap<String, String>,
}

/// A mod package entry from mod-packages.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModPackageInfo {
    pub id: String,
    pub manifest: ModPackageManifest,
    pub sha512: String,
    pub path: String,
}

/// The complete mod-packages.json structure
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModPackagesRegistry {
    #[serde(default)]
    pub client: Vec<ModPackageInfo>,
    #[serde(default)]
    pub server: Vec<ModPackageInfo>,
}

impl ModPackagesRegistry {
    /// Load mod-packages.json from STAM_HOME/mod-packages/mod-packages.json
    pub fn load_from_home(home_dir: &std::path::Path) -> Result<Self, String> {
        let packages_file = home_dir.join("mod-packages").join("mod-packages.json");

        if !packages_file.exists() {
            tracing::warn!("mod-packages.json not found at {:?}, using empty registry", packages_file);
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&packages_file)
            .map_err(|e| format!("Failed to read mod-packages.json: {}", e))?;

        let registry: ModPackagesRegistry = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse mod-packages.json: {}", e))?;

        tracing::info!(
            "Loaded mod-packages.json: {} client packages, {} server packages",
            registry.client.len(),
            registry.server.len()
        );

        Ok(registry)
    }

    /// Get packages filtered by side
    pub fn get_packages(&self, side: ModSide) -> &Vec<ModPackageInfo> {
        match side {
            ModSide::Client => &self.client,
            ModSide::Server => &self.server,
        }
    }
}

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
    /// Download URL for this mod (stam:// URI from server)
    pub download_url: Option<String>,
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
    /// Event dispatcher for handling system events
    event_dispatcher: EventDispatcher,
    /// Registry of mod packages from mod-packages.json (optional, server-only)
    mod_packages: Arc<RwLock<Option<ModPackagesRegistry>>>,
    /// Home directory path for mod packages
    home_dir: Arc<RwLock<Option<PathBuf>>>,
}

impl SystemApi {
    /// Create a new SystemApi with an empty mod list
    pub fn new() -> Self {
        Self {
            mods: Arc::new(RwLock::new(Vec::new())),
            event_dispatcher: EventDispatcher::new(),
            mod_packages: Arc::new(RwLock::new(None)),
            home_dir: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the mod packages registry (loaded from mod-packages.json)
    pub fn set_mod_packages(&self, registry: ModPackagesRegistry) {
        let mut packages = self.mod_packages.write().unwrap();
        *packages = Some(registry);
    }

    /// Set the home directory path
    pub fn set_home_dir(&self, path: PathBuf) {
        let mut home = self.home_dir.write().unwrap();
        *home = Some(path);
    }

    /// Get the home directory path
    pub fn get_home_dir(&self) -> Option<PathBuf> {
        let home = self.home_dir.read().unwrap();
        home.clone()
    }

    /// Get mod packages filtered by side
    pub fn get_mod_packages(&self, side: ModSide) -> Vec<ModPackageInfo> {
        let packages = self.mod_packages.read().unwrap();
        if let Some(ref registry) = *packages {
            registry.get_packages(side).clone()
        } else {
            Vec::new()
        }
    }

    /// Get the file path for a mod package by ID and side
    pub fn get_mod_package_file_path(&self, mod_id: &str, side: ModSide) -> Option<PathBuf> {
        let packages = self.mod_packages.read().unwrap();
        let home = self.home_dir.read().unwrap();

        if let (Some(registry), Some(home_path)) = (&*packages, &*home) {
            let package_list = registry.get_packages(side);
            if let Some(pkg) = package_list.iter().find(|p| p.id == mod_id) {
                return Some(home_path.join("mod-packages").join(&pkg.path));
            }
        }
        None
    }

    /// Get a reference to the event dispatcher
    pub fn event_dispatcher(&self) -> &EventDispatcher {
        &self.event_dispatcher
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
