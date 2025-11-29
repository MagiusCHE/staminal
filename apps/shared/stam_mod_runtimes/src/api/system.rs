//! System API for Mod Runtimes
//!
//! Provides access to mod information and system state.
//! The `system.get_mods()` function returns an array of mod info objects.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use super::events::EventDispatcher;

/// Request to attach (load and initialize) a mod at runtime
///
/// This is used by `system.attach_mod(mod_id)` to request the main loop
/// to load a mod that was previously installed via `install_mod_from_path`.
#[derive(Debug)]
pub struct AttachModRequest {
    /// The mod ID to attach
    pub mod_id: String,
    /// Channel to send the result back to the caller
    pub response_tx: oneshot::Sender<Result<(), String>>,
}

/// Request for graceful shutdown
///
/// This is used by `system.exit(code)` to request a graceful shutdown
/// instead of terminating the process immediately.
#[derive(Debug)]
pub struct ShutdownRequest {
    /// The exit code (0 = success, non-zero = error)
    pub exit_code: i32,
}

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

        tracing::debug!(
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

/// Minimal manifest structure for reading installed mod metadata
///
/// This is a subset of the full ModManifest schema, containing only
/// the fields needed for runtime registration.
#[derive(Debug, Clone, Deserialize)]
struct InstalledModManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub entry_point: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(rename = "type", default)]
    pub mod_type: Option<String>,
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
    /// Whether this mod exists locally (at the required version)
    pub exists: bool,
    /// Download URL for this mod (stam:// URI from server)
    pub download_url: Option<String>,
}

/// Information about the current game context (client-side only)
#[derive(Clone, Debug, Default)]
pub struct GameInfo {
    /// The game identifier
    pub id: String,
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
    /// Channel sender for attach mod requests (JS -> main loop)
    attach_request_tx: Arc<RwLock<Option<mpsc::Sender<AttachModRequest>>>>,
    /// Channel receiver for attach mod requests (main loop)
    attach_request_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<AttachModRequest>>>>,
    /// Channel sender for shutdown requests (JS -> main loop)
    shutdown_request_tx: Arc<RwLock<Option<mpsc::Sender<ShutdownRequest>>>>,
    /// Channel receiver for shutdown requests (main loop)
    shutdown_request_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<ShutdownRequest>>>>,
    /// Game information (client-side only, None on server)
    game_info: Arc<RwLock<Option<GameInfo>>>,
}

impl SystemApi {
    /// Create a new SystemApi with an empty mod list
    pub fn new() -> Self {
        // Create mpsc channel for attach requests (buffered with capacity 16)
        let (attach_tx, attach_rx) = mpsc::channel::<AttachModRequest>(16);
        // Create mpsc channel for shutdown requests (capacity 1 is enough)
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<ShutdownRequest>(1);

        Self {
            mods: Arc::new(RwLock::new(Vec::new())),
            event_dispatcher: EventDispatcher::new(),
            mod_packages: Arc::new(RwLock::new(None)),
            home_dir: Arc::new(RwLock::new(None)),
            attach_request_tx: Arc::new(RwLock::new(Some(attach_tx))),
            attach_request_rx: Arc::new(tokio::sync::Mutex::new(Some(attach_rx))),
            shutdown_request_tx: Arc::new(RwLock::new(Some(shutdown_tx))),
            shutdown_request_rx: Arc::new(tokio::sync::Mutex::new(Some(shutdown_rx))),
            game_info: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the game information (client-side only)
    ///
    /// This should be called on the client after connecting to a game server.
    /// On the server, this should NOT be called - leave game_info as None.
    pub fn set_game_info(&self, game_id: impl Into<String>) {
        let mut info = self.game_info.write().unwrap();
        *info = Some(GameInfo { id: game_id.into() });
    }

    /// Get the game information (client-side only)
    ///
    /// Returns None on server (game_info not set) or if not yet connected to a game.
    pub fn get_game_info(&self) -> Option<GameInfo> {
        let info = self.game_info.read().unwrap();
        info.clone()
    }

    /// Send a request to attach a mod and wait for the result
    ///
    /// This is called by the JS binding `system.attach_mod(mod_id)`.
    /// The request is sent to the main loop which will process it and respond.
    pub async fn request_attach_mod(&self, mod_id: String) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();

        let request = AttachModRequest {
            mod_id: mod_id.clone(),
            response_tx,
        };

        // Get the sender
        let tx = {
            let guard = self.attach_request_tx.read().unwrap();
            guard.clone()
        };

        let tx = tx.ok_or_else(|| "Attach request channel not available".to_string())?;

        // Send the request
        tx.send(request).await.map_err(|_| "Failed to send attach request".to_string())?;

        // Wait for the response
        response_rx.await.map_err(|_| "Attach request was cancelled".to_string())?
    }

    /// Take the attach request receiver (can only be called once)
    ///
    /// This is used by the main loop to receive and process attach requests.
    pub async fn take_attach_receiver(&self) -> Option<mpsc::Receiver<AttachModRequest>> {
        let mut guard = self.attach_request_rx.lock().await;
        guard.take()
    }

    /// Send a shutdown request
    ///
    /// This is called by `system.exit(code)` to request a graceful shutdown
    /// instead of terminating the process immediately.
    pub fn request_shutdown(&self, exit_code: i32) -> Result<(), String> {
        let tx = {
            let guard = self.shutdown_request_tx.read().unwrap();
            guard.clone()
        };

        let tx = tx.ok_or_else(|| "Shutdown request channel not available".to_string())?;

        // Use try_send since we don't want to block
        tx.try_send(ShutdownRequest { exit_code })
            .map_err(|e| format!("Failed to send shutdown request: {}", e))
    }

    /// Take the shutdown request receiver (can only be called once)
    ///
    /// This is used by the main loop to receive and process shutdown requests.
    pub async fn take_shutdown_receiver(&self) -> Option<mpsc::Receiver<ShutdownRequest>> {
        let mut guard = self.shutdown_request_rx.lock().await;
        guard.take()
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

    /// Get the mods directory path (home_dir/mods)
    pub fn get_mods_dir(&self) -> Option<PathBuf> {
        self.get_home_dir().map(|h| h.join("mods"))
    }

    /// Install a mod from a ZIP file
    ///
    /// Extracts the ZIP file contents to the mods directory under the specified mod_id.
    /// If the mod directory already exists, it is removed first.
    /// After extraction, reads the manifest and registers the mod with `loaded=false`.
    ///
    /// # Arguments
    /// * `zip_path` - Path to the ZIP file to extract
    /// * `mod_id` - The mod identifier (directory name)
    ///
    /// # Returns
    /// Ok(PathBuf) with the mod installation path on success, or Err(String) on failure
    pub fn install_mod_from_zip(&self, zip_path: &std::path::Path, mod_id: &str) -> Result<PathBuf, String> {
        let mods_dir = self.get_mods_dir()
            .ok_or_else(|| "Home directory not configured".to_string())?;

        let mod_target_dir = mods_dir.join(mod_id);

        tracing::debug!("Installing mod '{}' from {} to {}",
            mod_id,
            zip_path.display(),
            mod_target_dir.display());

        // Use the standalone extraction function
        extract_mod_zip(zip_path, &mod_target_dir)?;

        // Read manifest and register the mod with loaded=false
        // Check client/ subdirectory first, then root
        let client_manifest_path = mod_target_dir.join("client").join("manifest.json");
        let root_manifest_path = mod_target_dir.join("manifest.json");

        let manifest_path = if client_manifest_path.exists() {
            client_manifest_path
        } else if root_manifest_path.exists() {
            root_manifest_path
        } else {
            return Err(format!("No manifest.json found for mod '{}'", mod_id));
        };

        let manifest_content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("Failed to read manifest for mod '{}': {}", mod_id, e))?;

        let manifest: InstalledModManifest = serde_json::from_str(&manifest_content)
            .map_err(|e| format!("Failed to parse manifest for mod '{}': {}", mod_id, e))?;

        // Register the mod with loaded=false, exists=true (just installed)
        let mod_info = ModInfo {
            id: mod_id.to_string(),
            version: manifest.version,
            name: manifest.name,
            description: manifest.description,
            mod_type: manifest.mod_type,
            priority: manifest.priority,
            bootstrapped: false,
            loaded: false,
            exists: true,  // Just installed, so it exists locally
            download_url: None,
        };

        self.register_mod(mod_info);
        tracing::debug!("Mod '{}' installed and registered (loaded=false)", mod_id);

        Ok(mod_target_dir)
    }
}

impl Default for SystemApi {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract a mod from a ZIP file to a target directory
///
/// This is a standalone function that can be used without a SystemApi instance.
/// It extracts the ZIP file contents to the specified target directory.
/// If the target directory already exists, it is removed first.
///
/// # Arguments
/// * `zip_path` - Path to the ZIP file to extract
/// * `target_dir` - Target directory where the mod will be extracted
///
/// # Returns
/// Ok(()) on success, or Err(String) on failure
pub fn extract_mod_zip(zip_path: &std::path::Path, target_dir: &std::path::Path) -> Result<(), String> {
    tracing::debug!("Extracting ZIP {} to {}",
        zip_path.display(),
        target_dir.display());

    // Remove existing directory if present
    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir)
            .map_err(|e| format!("Failed to remove existing directory: {}", e))?;
    }

    // Create target directory
    std::fs::create_dir_all(target_dir)
        .map_err(|e| format!("Failed to create target directory: {}", e))?;

    // Open ZIP file
    let zip_file = std::fs::File::open(zip_path)
        .map_err(|e| format!("Failed to open ZIP file: {}", e))?;

    let mut archive = zip::ZipArchive::new(zip_file)
        .map_err(|e| format!("Failed to read ZIP archive: {}", e))?;

    // Extract all files
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry {}: {}", i, e))?;

        let outpath = target_dir.join(file.mangled_name());

        if file.is_dir() {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| format!("Failed to create directory {:?}: {}", outpath, e))?;
        } else {
            // Create parent directories if needed
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create parent directory {:?}: {}", parent, e))?;
                }
            }

            // Extract file
            let mut outfile = std::fs::File::create(&outpath)
                .map_err(|e| format!("Failed to create file {:?}: {}", outpath, e))?;

            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to write file {:?}: {}", outpath, e))?;
        }
    }

    tracing::debug!("ZIP extracted successfully to {}", target_dir.display());
    Ok(())
}
