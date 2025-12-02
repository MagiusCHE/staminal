//! API System for Mod Runtimes
//!
//! This module provides a flexible system for registering and providing APIs to mods.
//! APIs are runtime-agnostic - they define the logic, while runtime-specific bindings
//! (in adapters) expose them to the scripting languages.

pub mod console;
pub mod events;
pub mod graphic;
pub mod locale;
pub mod network;
pub mod process;
pub mod system;

pub use console::ConsoleApi;
pub use events::{EventDispatcher, EventHandler, EventKey, SystemEvents, RequestUriProtocol, RequestUri, UriResponse, SendEventRequest, TerminalKeyRequest, TerminalKeyResponse, GraphicEngineReadyRequest, GraphicEngineReadyResponse, GraphicEngineWindowClosedRequest, GraphicEngineWindowClosedResponse};
pub use graphic::{
    AlignItems, ColorValue, EdgeInsets, FlexDirection, FontConfig, FontInfo, GraphicCommand,
    GraphicEngine, GraphicEngineInfo, GraphicEngines, GraphicEvent, GraphicProxy,
    InitialWindowConfig, JustifyContent, KeyModifiers, MouseButton, PropertyValue, SizeValue,
    WidgetConfig, WidgetEventType, WidgetInfo, WidgetType, WindowConfig, WindowInfo,
    WindowPositionMode, EnableEngineRequest,
};
pub use locale::LocaleApi;
pub use network::{NetworkApi, NetworkConfig, DownloadResponse, parse_stam_uri, sanitize_uri};
pub use process::{ProcessApi, AppApi};
pub use system::{SystemApi, ModInfo, ModSide, ModPackageInfo, ModPackageManifest, ModPackagesRegistry, extract_mod_zip, AttachModRequest, ShutdownRequest, GameInfo};

use std::collections::HashMap;
use std::any::Any;

/// Registry for APIs that can be injected into runtimes
///
/// This allows client and server to configure which APIs are available to mods.
/// For example, the client might provide "process" and "client" APIs, while the
/// server might provide "server" and "database" APIs.
pub struct ApiRegistry {
    /// Map of API name to API provider instance
    apis: HashMap<String, Box<dyn Any>>,
}

impl ApiRegistry {
    /// Create a new empty API registry
    pub fn new() -> Self {
        Self {
            apis: HashMap::new(),
        }
    }

    /// Register an API provider
    ///
    /// # Arguments
    /// * `name` - Name of the API (e.g., "console", "process", "client")
    /// * `api` - The API implementation
    pub fn register<T: Any>(&mut self, name: impl Into<String>, api: T) {
        self.apis.insert(name.into(), Box::new(api));
    }

    /// Get an API by name
    ///
    /// # Arguments
    /// * `name` - Name of the API
    ///
    /// # Returns
    /// Option containing a reference to the API if found
    pub fn get<T: Any>(&self, name: &str) -> Option<&T> {
        self.apis.get(name)
            .and_then(|api| api.downcast_ref::<T>())
    }

    /// Check if an API is registered
    pub fn has(&self, name: &str) -> bool {
        self.apis.contains_key(name)
    }

    /// Get list of all registered API names
    pub fn list(&self) -> Vec<&str> {
        self.apis.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ApiRegistry {
    fn default() -> Self {
        Self::new()
    }
}
