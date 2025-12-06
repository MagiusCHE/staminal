//! Resource Management System
//!
//! This module provides a resource loading and caching system for the Staminal client.
//! Resources are loaded via `Resource.load()` and can be referenced by widgets using their alias.
//!
//! # Architecture
//!
//! The ResourceProxy is shared across all runtimes (JavaScript, Lua, future C#, etc.)
//! and maintains a unified cache of loaded resources.
//!
//! - **Graphic resources** (images, fonts, shaders, audio) are loaded via the GraphicEngine
//!   and tracked with an `EngineHandle` that keeps the underlying asset alive.
//! - **Non-graphic resources** (JSON, text, binary) are loaded directly into memory
//!   and stored in the ResourceProxy's data cache.
//!
//! # Loading Flow
//!
//! `Resource.load()` is **synchronous** - it does not return a Promise.
//! - If the resource is already loaded, returns the ResourceInfo immediately.
//! - If the resource is not loaded, it queues the resource for loading and returns `undefined`.
//! - The loading progress counters (`requested`/`loaded`) are updated synchronously.
//!
//! To wait for a specific resource to be loaded, use `Resource.whenLoaded(alias)`.
//!
//! # Thread Safety
//!
//! ResourceProxy is designed to be shared across threads using `Arc`.
//! Internal state is protected by `RwLock` and atomic operations.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::Notify;

// ============================================================================
// Resource Type Enum
// ============================================================================

/// Types of resources that can be loaded
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceType {
    /// Image files (.png, .jpg, .jpeg, .gif, .webp, .bmp, etc.)
    Image,
    /// Audio files (.mp3, .ogg, .wav, .flac)
    Audio,
    /// Video files (.mp4, .webm)
    Video,
    /// Shader files (.wgsl, .glsl)
    Shader,
    /// Font files (.ttf, .otf)
    Font,
    /// 3D model files (.gltf, .glb)
    Model3D,
    /// JSON files (.json)
    Json,
    /// Text files (.txt, .md, .xml, .csv)
    Text,
    /// Binary files (any other extension)
    Binary,
}

impl ResourceType {
    /// Determine resource type from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            // Images
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tga" | "tiff" | "ico" | "hdr"
            | "exr" | "ktx2" | "dds" | "pnm" | "pam" | "pbm" | "pgm" | "ppm" | "qoi" | "basis" => {
                ResourceType::Image
            }
            // Audio
            "mp3" | "ogg" | "wav" | "flac" => ResourceType::Audio,
            // Video
            "mp4" | "webm" | "avi" => ResourceType::Video,
            // Shaders
            "wgsl" | "glsl" => ResourceType::Shader,
            // Fonts
            "ttf" | "otf" | "woff" | "woff2" => ResourceType::Font,
            // 3D Models
            "gltf" | "glb" => ResourceType::Model3D,
            // JSON
            "json" => ResourceType::Json,
            // Text
            "txt" | "md" | "xml" | "csv" | "yaml" | "yml" | "toml" => ResourceType::Text,
            // Default to binary
            _ => ResourceType::Binary,
        }
    }

    /// Check if this resource type is handled by the GraphicEngine
    pub fn is_graphic_resource(&self) -> bool {
        matches!(
            self,
            ResourceType::Image
                | ResourceType::Audio
                | ResourceType::Shader
                | ResourceType::Font
                | ResourceType::Model3D
        )
    }
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceType::Image => write!(f, "image"),
            ResourceType::Audio => write!(f, "audio"),
            ResourceType::Video => write!(f, "video"),
            ResourceType::Shader => write!(f, "shader"),
            ResourceType::Font => write!(f, "font"),
            ResourceType::Model3D => write!(f, "model3d"),
            ResourceType::Json => write!(f, "json"),
            ResourceType::Text => write!(f, "text"),
            ResourceType::Binary => write!(f, "binary"),
        }
    }
}

impl ResourceType {
    /// Get string representation for JavaScript
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceType::Image => "image",
            ResourceType::Audio => "audio",
            ResourceType::Video => "video",
            ResourceType::Shader => "shader",
            ResourceType::Font => "font",
            ResourceType::Model3D => "model3d",
            ResourceType::Json => "json",
            ResourceType::Text => "text",
            ResourceType::Binary => "binary",
        }
    }
}

// ============================================================================
// Resource State Enum
// ============================================================================

/// State of a resource in the loading pipeline
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceState {
    /// Resource is being loaded
    Loading,
    /// Resource has been successfully loaded
    Loaded,
    /// Resource failed to load
    Error,
}

impl std::fmt::Display for ResourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceState::Loading => write!(f, "loading"),
            ResourceState::Loaded => write!(f, "loaded"),
            ResourceState::Error => write!(f, "error"),
        }
    }
}

impl ResourceState {
    /// Get string representation for JavaScript
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceState::Loading => "loading",
            ResourceState::Loaded => "loaded",
            ResourceState::Error => "error",
        }
    }
}

// ============================================================================
// Engine Handle
// ============================================================================

/// Opaque handle representing a resource in the GraphicEngine
///
/// This handle keeps the underlying asset alive in the engine's cache.
/// When the handle is dropped (via ResourceProxy.remove()), the engine
/// can garbage collect the asset.
#[derive(Clone, Debug)]
pub enum EngineHandle {
    /// Handle for Bevy engine resources
    Bevy {
        /// Unique ID for lookup in Bevy's ResourceRegistry
        asset_id: u64,
    },
    // Future: other engines
    // Vulkan { ... },
    // WebGL { ... },
}

impl EngineHandle {
    /// Get the asset ID for Bevy handles
    pub fn bevy_asset_id(&self) -> Option<u64> {
        match self {
            EngineHandle::Bevy { asset_id } => Some(*asset_id),
        }
    }
}

// ============================================================================
// Resource Data (for non-graphic resources)
// ============================================================================

/// Data storage for non-graphic resources loaded into memory
#[derive(Clone, Debug)]
pub enum ResourceData {
    /// JSON data
    Json(serde_json::Value),
    /// Text data
    Text(String),
    /// Binary data
    Binary(Vec<u8>),
}

// ============================================================================
// Resource Entry
// ============================================================================

/// Complete entry for a resource in the cache
#[derive(Clone, Debug)]
pub struct ResourceEntry {
    /// Original path provided to load()
    pub path: String,
    /// Resolved absolute path
    pub resolved_path: String,
    /// Type of resource
    pub resource_type: ResourceType,
    /// Current loading state
    pub state: ResourceState,
    /// Handle to graphic engine resource (for graphic resources)
    pub engine_handle: Option<EngineHandle>,
    /// In-memory data (for non-graphic resources)
    pub data: Option<ResourceData>,
    /// Size in bytes (if known)
    pub size: Option<u64>,
    /// Error message (if state == Error)
    pub error: Option<String>,
}

// ============================================================================
// Resource Info (for API responses)
// ============================================================================

/// Information about a loaded resource (returned to JavaScript)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    /// Resource alias (the ID used to reference it)
    pub alias: String,
    /// Original path provided to load()
    pub path: String,
    /// Resolved absolute path
    pub resolved_path: String,
    /// Type of resource
    #[serde(rename = "type")]
    pub resource_type: ResourceType,
    /// Current loading state
    pub state: ResourceState,
    /// Size in bytes (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Error message (if state == Error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ResourceInfo {
    /// Create ResourceInfo from ResourceEntry
    pub fn from_entry(alias: &str, entry: &ResourceEntry) -> Self {
        Self {
            alias: alias.to_string(),
            path: entry.path.clone(),
            resolved_path: entry.resolved_path.clone(),
            resource_type: entry.resource_type,
            state: entry.state.clone(),
            size: entry.size,
            error: entry.error.clone(),
        }
    }
}

// ============================================================================
// Loading State
// ============================================================================

/// Loading state - pre-calculated counters updated on each operation
///
/// This struct is updated incrementally whenever resources are loaded,
/// completed, or unloaded. It does NOT calculate values on-demand.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct LoadingState {
    /// Number of resources requested (passed to load())
    pub requested: u32,
    /// Number of resources successfully loaded
    pub loaded: u32,
}

// ============================================================================
// Resource Queue Request
// ============================================================================

/// A request to load a resource, queued for async processing
#[derive(Clone, Debug)]
pub struct QueuedLoadRequest {
    /// The resolved path to the resource file
    pub resolved_path: String,
    /// The unique alias for this resource
    pub alias: String,
    /// The type of resource to load
    pub resource_type: ResourceType,
    /// The asset ID assigned by ResourceProxy
    pub asset_id: u64,
}

// ============================================================================
// Resource Proxy
// ============================================================================

/// Central proxy for resource management
///
/// This struct is shared across all mod contexts and ALL scripting runtimes
/// (JavaScript, Lua, C#, etc.). It provides a unified interface for resource
/// loading and caching.
///
/// # Thread Safety
///
/// ResourceProxy is designed to be shared across threads using `Arc`.
/// Internal state is protected by `RwLock` and atomic operations.
pub struct ResourceProxy {
    /// Resource cache: resource_id -> ResourceEntry
    resources: Arc<RwLock<HashMap<String, ResourceEntry>>>,

    /// Loading state counters (pre-calculated, not computed on-demand)
    loading_requested: AtomicU32,
    loading_loaded: AtomicU32,

    /// Counter to generate unique asset IDs for engine handles
    next_asset_id: AtomicU64,

    /// Flag: resource proxy is available (client-only)
    available: bool,

    /// Queue of resources pending to be loaded
    load_queue: Arc<RwLock<VecDeque<QueuedLoadRequest>>>,

    /// Notification for when new items are added to the queue
    queue_notify: Arc<Notify>,

    /// Notification channels for individual resources (alias -> list of Notify)
    /// When a resource is loaded, all waiters are notified
    resource_waiters: Arc<RwLock<HashMap<String, Arc<Notify>>>>,
}

impl ResourceProxy {
    /// Create a new ResourceProxy for the client
    pub fn new_client() -> Self {
        Self {
            resources: Arc::new(RwLock::new(HashMap::new())),
            loading_requested: AtomicU32::new(0),
            loading_loaded: AtomicU32::new(0),
            next_asset_id: AtomicU64::new(1),
            available: true,
            load_queue: Arc::new(RwLock::new(VecDeque::new())),
            queue_notify: Arc::new(Notify::new()),
            resource_waiters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a stub ResourceProxy for the server
    ///
    /// All operations will return errors indicating they are client-only.
    pub fn new_server_stub() -> Self {
        Self {
            resources: Arc::new(RwLock::new(HashMap::new())),
            loading_requested: AtomicU32::new(0),
            loading_loaded: AtomicU32::new(0),
            next_asset_id: AtomicU64::new(1),
            available: false,
            load_queue: Arc::new(RwLock::new(VecDeque::new())),
            queue_notify: Arc::new(Notify::new()),
            resource_waiters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if the resource proxy is available
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Generate a new unique asset ID for engine handles
    pub fn next_asset_id(&self) -> u64 {
        self.next_asset_id.fetch_add(1, Ordering::SeqCst)
    }

    // ========================================================================
    // Resource Registration and Management
    // ========================================================================

    /// Registers a new resource in the cache
    ///
    /// Automatically increments loading_state.requested
    pub fn register(&self, resource_id: &str, entry: ResourceEntry) -> Result<(), String> {
        let mut resources = self.resources.write().unwrap();

        if resources.contains_key(resource_id) {
            return Err(format!(
                "Resource with ID '{}' already exists. Use forceReload to replace it.",
                resource_id
            ));
        }

        resources.insert(resource_id.to_string(), entry);

        // Increment requested counter
        self.loading_requested.fetch_add(1, Ordering::SeqCst);

        Ok(())
    }

    /// Checks if a resource exists in cache
    pub fn exists(&self, resource_id: &str) -> bool {
        self.resources.read().unwrap().contains_key(resource_id)
    }

    /// Gets the complete entry for a resource
    pub fn get(&self, resource_id: &str) -> Option<ResourceEntry> {
        self.resources.read().unwrap().get(resource_id).cloned()
    }

    /// Gets only the engine handle for a resource (for widget use)
    pub fn get_engine_handle(&self, resource_id: &str) -> Option<EngineHandle> {
        self.resources
            .read()
            .unwrap()
            .get(resource_id)
            .and_then(|e| e.engine_handle.clone())
    }

    /// Updates an existing resource (for forceReload)
    pub fn update(&self, resource_id: &str, entry: ResourceEntry) -> Result<(), String> {
        let mut resources = self.resources.write().unwrap();

        if !resources.contains_key(resource_id) {
            return Err(format!("Resource with ID '{}' does not exist", resource_id));
        }

        // Get old state to adjust counters
        let old_state = resources.get(resource_id).map(|e| e.state.clone());

        // If old resource was loaded, decrement loaded counter
        if old_state == Some(ResourceState::Loaded) {
            self.loading_loaded.fetch_sub(1, Ordering::SeqCst);
        }

        // If new entry is already loaded, increment loaded counter
        if entry.state == ResourceState::Loaded {
            self.loading_loaded.fetch_add(1, Ordering::SeqCst);
        }

        resources.insert(resource_id.to_string(), entry);

        Ok(())
    }

    /// Marks a resource as loaded
    ///
    /// Automatically increments loading_state.loaded and notifies waiters.
    pub fn mark_loaded(&self, resource_id: &str) -> Result<(), String> {
        {
            let mut resources = self.resources.write().unwrap();

            let entry = resources
                .get_mut(resource_id)
                .ok_or_else(|| format!("Resource with ID '{}' does not exist", resource_id))?;

            if entry.state != ResourceState::Loading {
                return Err(format!(
                    "Resource '{}' is not in loading state (current: {})",
                    resource_id, entry.state
                ));
            }

            entry.state = ResourceState::Loaded;

            // Increment loaded counter
            self.loading_loaded.fetch_add(1, Ordering::SeqCst);
        }

        // Notify any waiters for this resource
        self.notify_waiters(resource_id);

        Ok(())
    }

    /// Marks a resource as failed
    ///
    /// Does NOT modify loading_state (stays in requested for potential retry).
    /// Notifies waiters so they can handle the error.
    pub fn mark_failed(&self, resource_id: &str, error: String) -> Result<(), String> {
        {
            let mut resources = self.resources.write().unwrap();

            let entry = resources
                .get_mut(resource_id)
                .ok_or_else(|| format!("Resource with ID '{}' does not exist", resource_id))?;

            entry.state = ResourceState::Error;
            entry.error = Some(error);

            // Do NOT modify counters - stays in requested for retry
        }

        // Notify any waiters for this resource (they'll see the error state)
        self.notify_waiters(resource_id);

        Ok(())
    }

    /// Notify all waiters for a specific resource
    fn notify_waiters(&self, resource_id: &str) {
        let waiters = self.resource_waiters.read().unwrap();
        if let Some(notify) = waiters.get(resource_id) {
            notify.notify_waiters();
        }
    }

    /// Sets the engine handle for a resource
    pub fn set_engine_handle(
        &self,
        resource_id: &str,
        handle: EngineHandle,
    ) -> Result<(), String> {
        let mut resources = self.resources.write().unwrap();

        let entry = resources
            .get_mut(resource_id)
            .ok_or_else(|| format!("Resource with ID '{}' does not exist", resource_id))?;

        entry.engine_handle = Some(handle);

        Ok(())
    }

    /// Sets the data for a non-graphic resource
    pub fn set_data(&self, resource_id: &str, data: ResourceData) -> Result<(), String> {
        let mut resources = self.resources.write().unwrap();

        let entry = resources
            .get_mut(resource_id)
            .ok_or_else(|| format!("Resource with ID '{}' does not exist", resource_id))?;

        entry.data = Some(data);

        Ok(())
    }

    /// Removes a resource from cache
    ///
    /// Updates loading_state based on resource state:
    /// - If loading: requested -= 1
    /// - If loaded: requested -= 1, loaded -= 1
    ///
    /// Returns the removed entry (if any) so caller can release engine handle
    pub fn remove(&self, resource_id: &str) -> Option<ResourceEntry> {
        let mut resources = self.resources.write().unwrap();

        if let Some(entry) = resources.remove(resource_id) {
            // Update counters based on state
            self.loading_requested.fetch_sub(1, Ordering::SeqCst);

            if entry.state == ResourceState::Loaded {
                self.loading_loaded.fetch_sub(1, Ordering::SeqCst);
            }

            Some(entry)
        } else {
            None
        }
    }

    /// Removes all resources from cache
    ///
    /// Resets loading_state to { requested: 0, loaded: 0 }
    ///
    /// Returns all removed entries so caller can release engine handles
    pub fn clear(&self) -> Vec<(String, ResourceEntry)> {
        let mut resources = self.resources.write().unwrap();

        let entries: Vec<(String, ResourceEntry)> = resources.drain().collect();

        // Reset counters
        self.loading_requested.store(0, Ordering::SeqCst);
        self.loading_loaded.store(0, Ordering::SeqCst);

        entries
    }

    /// Lists all resource IDs in cache
    pub fn list(&self) -> Vec<String> {
        self.resources.read().unwrap().keys().cloned().collect()
    }

    /// Gets info about a resource (for JavaScript API)
    pub fn get_info(&self, resource_id: &str) -> Option<ResourceInfo> {
        self.resources
            .read()
            .unwrap()
            .get(resource_id)
            .map(|entry| ResourceInfo::from_entry(resource_id, entry))
    }

    // ========================================================================
    // Loading Progress
    // ========================================================================

    /// Gets the current loading state
    ///
    /// Returns pre-calculated data, O(1) complexity
    pub fn get_loading_progress(&self) -> LoadingState {
        LoadingState {
            requested: self.loading_requested.load(Ordering::SeqCst),
            loaded: self.loading_loaded.load(Ordering::SeqCst),
        }
    }

    /// Checks if all requested resources have been loaded
    ///
    /// Returns true if requested == loaded (all done) or if requested == 0 (nothing pending)
    pub fn is_loading_completed(&self) -> bool {
        let requested = self.loading_requested.load(Ordering::SeqCst);
        let loaded = self.loading_loaded.load(Ordering::SeqCst);

        requested == 0 || requested == loaded
    }

    // ========================================================================
    // Utility Methods
    // ========================================================================

    /// Check if a resource is loaded and ready to use
    pub fn is_loaded(&self, resource_id: &str) -> bool {
        self.resources
            .read()
            .unwrap()
            .get(resource_id)
            .map(|e| e.state == ResourceState::Loaded)
            .unwrap_or(false)
    }

    // ========================================================================
    // High-Level API (called by JavaScript bindings)
    // ========================================================================

    /// Queue a resource for loading (SYNCHRONOUS)
    ///
    /// This method is the main entry point for loading resources. It is fully synchronous:
    /// - If the resource is already loaded, returns `Ok(Some(ResourceInfo))`
    /// - If the resource needs loading, queues it and returns `Ok(None)` (meaning "loading")
    /// - The `requested` counter is incremented immediately
    ///
    /// Use `when_loaded(alias)` to wait for the resource to finish loading.
    ///
    /// # Arguments
    /// * `resolved_path` - The resolved path to the resource file
    /// * `alias` - Unique alias for this resource
    /// * `resource_type` - The type of resource
    /// * `force_reload` - If true, reload even if cached
    ///
    /// # Returns
    /// * `Ok(Some(info))` - Resource is already loaded
    /// * `Ok(None)` - Resource has been queued for loading
    /// * `Err(String)` - Error (e.g., server-side, duplicate alias)
    pub fn queue_load(
        &self,
        resolved_path: &str,
        alias: &str,
        resource_type: ResourceType,
        force_reload: bool,
    ) -> Result<Option<ResourceInfo>, String> {
        if !self.available {
            return Err(
                "Resource.load() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        // Check if already loaded (and not force_reload)
        if !force_reload && self.exists(alias) {
            if let Some(entry) = self.get(alias) {
                if entry.state == ResourceState::Loaded {
                    // Already loaded, return the info
                    return Ok(Some(ResourceInfo::from_entry(alias, &entry)));
                }
                // If loading or error, don't re-queue
                if entry.state == ResourceState::Loading {
                    // Already in queue, return None to indicate "loading"
                    return Ok(None);
                }
            }
        }

        // If force reload, remove the old entry first
        if force_reload && self.exists(alias) {
            let _ = self.remove(alias);
        }

        // Generate asset ID for this resource
        let asset_id = self.next_asset_id();

        // Create initial entry with Loading state
        let entry = ResourceEntry {
            path: resolved_path.to_string(),
            resolved_path: resolved_path.to_string(),
            resource_type,
            state: ResourceState::Loading,
            engine_handle: None,
            data: None,
            size: None,
            error: None,
        };

        // Register in cache - this increments requested counter SYNCHRONOUSLY
        self.register(alias, entry)?;

        // Create a waiter for this resource (if someone calls when_loaded)
        {
            let mut waiters = self.resource_waiters.write().unwrap();
            waiters.entry(alias.to_string()).or_insert_with(|| Arc::new(Notify::new()));
        }

        // Add to load queue
        {
            let mut queue = self.load_queue.write().unwrap();
            queue.push_back(QueuedLoadRequest {
                resolved_path: resolved_path.to_string(),
                alias: alias.to_string(),
                resource_type,
                asset_id,
            });
        }

        // Notify the queue processor that there's work
        self.queue_notify.notify_one();

        // Return None to indicate "queued for loading"
        Ok(None)
    }

    /// Wait for a specific resource to be loaded
    ///
    /// This method waits asynchronously until the specified resource
    /// transitions from `Loading` to `Loaded` (or `Error`).
    ///
    /// # Arguments
    /// * `alias` - The resource alias to wait for
    ///
    /// # Returns
    /// * `Ok(ResourceInfo)` - Resource loaded successfully
    /// * `Err(String)` - Resource failed to load or not found
    pub async fn when_loaded(&self, alias: &str) -> Result<ResourceInfo, String> {
        if !self.available {
            return Err(
                "Resource.whenLoaded() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        // Fast path: check if already loaded
        if let Some(entry) = self.get(alias) {
            match entry.state {
                ResourceState::Loaded => {
                    return Ok(ResourceInfo::from_entry(alias, &entry));
                }
                ResourceState::Error => {
                    return Err(entry.error.unwrap_or_else(|| "Unknown error".to_string()));
                }
                ResourceState::Loading => {
                    // Continue to wait
                }
            }
        } else {
            return Err(format!("Resource '{}' not found. Call Resource.load() first.", alias));
        }

        // Get or create a waiter for this resource
        let notify = {
            let mut waiters = self.resource_waiters.write().unwrap();
            waiters.entry(alias.to_string())
                .or_insert_with(|| Arc::new(Notify::new()))
                .clone()
        };

        // Wait for notification
        notify.notified().await;

        // Check the result
        if let Some(entry) = self.get(alias) {
            match entry.state {
                ResourceState::Loaded => {
                    Ok(ResourceInfo::from_entry(alias, &entry))
                }
                ResourceState::Error => {
                    Err(entry.error.unwrap_or_else(|| "Unknown error".to_string()))
                }
                ResourceState::Loading => {
                    // Shouldn't happen, but handle gracefully
                    Err(format!("Resource '{}' is still loading", alias))
                }
            }
        } else {
            Err(format!("Resource '{}' was removed while loading", alias))
        }
    }

    /// Wait for all requested resources to be loaded
    ///
    /// This method waits asynchronously until all resources that have been
    /// queued via `queue_load()` are either loaded or failed.
    ///
    /// # Returns
    /// * `Ok(())` - All resources loaded successfully
    /// * `Err(Vec<(String, String)>)` - List of (alias, error) for failed resources
    pub async fn when_loaded_all(&self) -> Result<(), Vec<(String, String)>> {
        if !self.available {
            return Err(vec![(
                "".to_string(),
                "Resource.whenLoadedAll() is not available on the server. This method is client-only.".to_string(),
            )]);
        }

        // Fast path: if already completed, return immediately
        if self.is_loading_completed() {
            // Check for any errors
            let errors = self.collect_errors();
            if errors.is_empty() {
                return Ok(());
            } else {
                return Err(errors);
            }
        }

        // Wait for all resources by polling with a small interval
        // We use a simple polling approach because we need to wait for ALL resources,
        // not just one specific resource
        loop {
            // Small sleep to avoid busy-waiting
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            if self.is_loading_completed() {
                let errors = self.collect_errors();
                if errors.is_empty() {
                    return Ok(());
                } else {
                    return Err(errors);
                }
            }
        }
    }

    /// Collect all resources that are in error state
    fn collect_errors(&self) -> Vec<(String, String)> {
        let resources = self.resources.read().unwrap();
        resources
            .iter()
            .filter(|(_, entry)| entry.state == ResourceState::Error)
            .map(|(alias, entry)| {
                (
                    alias.clone(),
                    entry.error.clone().unwrap_or_else(|| "Unknown error".to_string()),
                )
            })
            .collect()
    }

    // ========================================================================
    // Queue Processing
    // ========================================================================

    /// Take the next item from the load queue
    ///
    /// Returns `None` if the queue is empty.
    pub fn take_from_queue(&self) -> Option<QueuedLoadRequest> {
        self.load_queue.write().unwrap().pop_front()
    }

    /// Get the queue notification handle for waiting on new items
    ///
    /// The queue processor should call `notified().await` on this
    /// to wait for new items without busy-looping.
    pub fn get_queue_notify(&self) -> Arc<Notify> {
        self.queue_notify.clone()
    }

    /// Check if the load queue is empty
    pub fn is_queue_empty(&self) -> bool {
        self.load_queue.read().unwrap().is_empty()
    }

    /// Get the current queue length
    pub fn queue_len(&self) -> usize {
        self.load_queue.read().unwrap().len()
    }

    /// Process a single queued load request
    ///
    /// This is called by the queue processor to actually load a resource.
    /// It loads via GraphicEngine if available, otherwise uses internal cache.
    ///
    /// NOTE: For graphic resources, this method does NOT call `mark_loaded()`.
    /// The graphic engine loads assets asynchronously and will send a
    /// `ResourceLoaded` event when the asset is actually ready. The event
    /// handler should call `mark_loaded()` at that point.
    pub async fn process_load_request(
        &self,
        request: &QueuedLoadRequest,
        graphic_proxy: &super::GraphicProxy,
    ) -> Result<ResourceInfo, String> {
        let QueuedLoadRequest {
            resolved_path,
            alias,
            resource_type,
            asset_id,
        } = request;

        // Load based on resource type
        if resource_type.is_graphic_resource()
            && is_bevy_supported_extension(resolved_path.rsplit('.').next().unwrap_or(""))
        {
            // Load via graphic engine
            // NOTE: The engine returns ResourceState::Loading, not Loaded.
            // The actual loading happens async in the engine, and a ResourceLoaded
            // event will be sent when the asset is ready.
            match graphic_proxy
                .load_resource(
                    resolved_path.clone(),
                    alias.clone(),
                    *resource_type,
                    *asset_id,
                    false, // force_reload already handled in queue_load
                )
                .await
            {
                Ok(info) => {
                    // Update entry with engine handle
                    self.set_engine_handle(alias, EngineHandle::Bevy { asset_id: *asset_id })?;

                    // DO NOT call mark_loaded() here!
                    // The graphic engine loads assets asynchronously.
                    // When the asset is ready, it will send a ResourceLoaded event.
                    // The event handler will call mark_loaded() at that point.

                    // Return info with current state (Loading)
                    Ok(info)
                }
                Err(e) => {
                    self.mark_failed(alias, e.clone())?;
                    Err(e)
                }
            }
        } else {
            // Non-graphic resource - load into memory
            self.mark_failed(
                alias,
                "Non-graphic resource loading not yet implemented".to_string(),
            )?;
            Err("Non-graphic resource loading not yet implemented".to_string())
        }
    }

    // ========================================================================
    // Legacy API (for backward compatibility - will be deprecated)
    // ========================================================================

    /// Load a resource into the cache (async convenience method)
    ///
    /// This method combines queue_load + when_loaded for cases where
    /// you want to wait for the resource synchronously.
    ///
    /// **Note:** Prefer using `queue_load()` + `when_loaded()` separately
    /// for better control over loading flow.
    #[deprecated(note = "Use queue_load() + when_loaded() instead")]
    pub async fn load_resource(
        &self,
        resolved_path: &str,
        alias: &str,
        resource_type: ResourceType,
        force_reload: bool,
        graphic_proxy: &super::GraphicProxy,
    ) -> Result<ResourceInfo, String> {
        // Queue the load
        match self.queue_load(resolved_path, alias, resource_type, force_reload)? {
            Some(info) => Ok(info), // Already loaded
            None => {
                // Process immediately (for backward compatibility)
                if let Some(request) = self.take_from_queue() {
                    self.process_load_request(&request, graphic_proxy).await
                } else {
                    self.when_loaded(alias).await
                }
            }
        }
    }

    /// Unload a resource from the cache
    pub async fn unload_resource(
        &self,
        alias: &str,
        graphic_proxy: &super::GraphicProxy,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "Resource.unload() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        // Remove from cache and get the entry
        let entry = self
            .remove(alias)
            .ok_or_else(|| format!("Resource '{}' not found", alias))?;

        // Unload from graphic engine if applicable
        if let Some(handle) = entry.engine_handle {
            if let Some(asset_id) = handle.bevy_asset_id() {
                graphic_proxy.unload_resource(asset_id).await?;
            }
        }

        Ok(())
    }

    /// Unload all resources from the cache
    pub async fn unload_all_resources(
        &self,
        graphic_proxy: &super::GraphicProxy,
    ) -> Result<(), String> {
        if !self.available {
            return Err(
                "Resource.unloadAll() is not available on the server. This method is client-only."
                    .to_string(),
            );
        }

        // Clear cache and get all entries
        let entries = self.clear();

        // Unload all from graphic engine
        graphic_proxy.unload_all_resources().await?;

        // Log how many were unloaded
        tracing::debug!("Unloaded {} resources", entries.len());

        Ok(())
    }

    /// Get information about a resource (wrapper for JavaScript API)
    pub fn get_resource_info(&self, alias: &str) -> Option<ResourceInfo> {
        self.get_info(alias)
    }

    /// Check if a resource is loaded (wrapper for JavaScript API)
    pub fn is_resource_loaded(&self, alias: &str) -> bool {
        self.is_loaded(alias)
    }

    /// Get current loading state (wrapper for JavaScript API)
    pub fn get_loading_state(&self) -> LoadingState {
        self.get_loading_progress()
    }

    /// List all loaded resource aliases
    pub fn list_loaded_resources(&self) -> Vec<String> {
        self.list()
    }
}

// ResourceProxy is Send + Sync because all internal state is protected
unsafe impl Send for ResourceProxy {}
unsafe impl Sync for ResourceProxy {}

// ============================================================================
// Extension → ResourceType mapping for Bevy
// ============================================================================

/// Returns the set of file extensions that Bevy can handle natively
///
/// Resources with these extensions will be loaded via the GraphicEngine.
/// Resources with other extensions will be loaded into ResourceProxy's data cache.
pub fn bevy_supported_extensions() -> HashMap<&'static str, ResourceType> {
    let mut map = HashMap::new();

    // Images (Handle<Image>)
    // Default enabled
    map.insert("png", ResourceType::Image);
    map.insert("hdr", ResourceType::Image);
    map.insert("ktx2", ResourceType::Image);
    // Optional features
    map.insert("jpg", ResourceType::Image);
    map.insert("jpeg", ResourceType::Image);
    map.insert("bmp", ResourceType::Image);
    map.insert("dds", ResourceType::Image);
    map.insert("tga", ResourceType::Image);
    map.insert("tiff", ResourceType::Image);
    map.insert("webp", ResourceType::Image);
    map.insert("gif", ResourceType::Image);
    map.insert("ico", ResourceType::Image);
    map.insert("exr", ResourceType::Image);
    map.insert("pnm", ResourceType::Image);
    map.insert("pam", ResourceType::Image);
    map.insert("pbm", ResourceType::Image);
    map.insert("pgm", ResourceType::Image);
    map.insert("ppm", ResourceType::Image);
    map.insert("qoi", ResourceType::Image);
    map.insert("basis", ResourceType::Image);

    // Fonts (Handle<Font>)
    map.insert("ttf", ResourceType::Font);
    map.insert("otf", ResourceType::Font);

    // Audio (Handle<AudioSource>)
    map.insert("ogg", ResourceType::Audio);
    map.insert("wav", ResourceType::Audio);
    map.insert("mp3", ResourceType::Audio);
    map.insert("flac", ResourceType::Audio);

    // Shaders (Handle<Shader>)
    map.insert("wgsl", ResourceType::Shader);

    // 3D Models (Handle<Gltf>)
    map.insert("gltf", ResourceType::Model3D);
    map.insert("glb", ResourceType::Model3D);

    map
}

/// Check if an extension is supported by Bevy's AssetServer
pub fn is_bevy_supported_extension(ext: &str) -> bool {
    bevy_supported_extensions().contains_key(ext.to_lowercase().as_str())
}
