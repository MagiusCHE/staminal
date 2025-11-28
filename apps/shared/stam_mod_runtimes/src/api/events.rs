//! Event System API for Mod Runtimes
//!
//! Provides an event dispatcher system that allows mods to register handlers
//! for various system events. Handlers are called sequentially by priority.
//!
//! # Architecture
//!
//! - **Registration**: Mods register handlers during `onAttach()`.
//! - **Persistence**: Registrations persist until application close or `onDetach()`.
//! - **Dispatch**: Handlers are executed sequentially, respecting priority (lower first).
//!
//! # Event Types
//!
//! Events can be either:
//! - **System events**: Predefined events like `RequestUri` (represented as enum)
//! - **Custom events**: User-defined events like `"AppStart"` (represented as strings)
//!
//! Both use the same registration and dispatch mechanism through `EventKey`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, oneshot};

/// System events that mods can register handlers for
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SystemEvents {
    /// URI request event - triggered when a stam:// or http:// request is made
    /// Additional args: protocol filter, route filter
    RequestUri = 1,
}

impl SystemEvents {
    /// Convert from u32 (for JavaScript interop)
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(SystemEvents::RequestUri),
            _ => None,
        }
    }

    /// Convert to u32 (for JavaScript interop)
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    /// Convert to string key for unified event handling
    pub fn to_key(&self) -> String {
        match self {
            SystemEvents::RequestUri => "system:RequestUri".to_string(),
        }
    }

    /// Try to parse from a string key
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "system:RequestUri" => Some(SystemEvents::RequestUri),
            _ => None,
        }
    }
}

/// Unified event key that can be either a system event or a custom event name
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventKey {
    /// System event (predefined)
    System(SystemEvents),
    /// Custom event (user-defined string)
    Custom(String),
}

impl EventKey {
    /// Create a key from a system event
    pub fn system(event: SystemEvents) -> Self {
        EventKey::System(event)
    }

    /// Create a key from a custom event name
    pub fn custom(name: impl Into<String>) -> Self {
        EventKey::Custom(name.into())
    }

    /// Convert to a string representation for internal storage
    pub fn to_string_key(&self) -> String {
        match self {
            EventKey::System(event) => event.to_key(),
            EventKey::Custom(name) => format!("custom:{}", name),
        }
    }

    /// Parse from JavaScript: either a u32 (system event) or a string (custom event)
    pub fn from_js_value(value: u32, string_value: Option<&str>) -> Option<Self> {
        // If value is 0 and we have a string, it's a custom event
        if value == 0 {
            if let Some(name) = string_value {
                return Some(EventKey::Custom(name.to_string()));
            }
            return None;
        }
        // Otherwise try to parse as system event
        SystemEvents::from_u32(value).map(EventKey::System)
    }

    /// Check if this is a custom event
    pub fn is_custom(&self) -> bool {
        matches!(self, EventKey::Custom(_))
    }

    /// Get the custom event name if this is a custom event
    pub fn custom_name(&self) -> Option<&str> {
        match self {
            EventKey::Custom(name) => Some(name),
            EventKey::System(_) => None,
        }
    }
}

/// Request to send/dispatch an event from JavaScript
///
/// This is used by `system.send_event(event_name, ...args)` to trigger
/// handlers registered for custom events.
#[derive(Debug)]
pub struct SendEventRequest {
    /// The event name to dispatch
    pub event_name: String,
    /// Arguments to pass to handlers (JSON-serialized)
    pub args: Vec<String>,
    /// Channel to send the result back to the caller
    pub response_tx: oneshot::Sender<Result<(), String>>,
}

/// Protocol filter for RequestUri events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum RequestUriProtocol {
    /// Match stam:// protocol only
    Stam = 1,
    /// Match http:// and https:// protocols only
    Http = 2,
    /// Match all protocols (default)
    #[default]
    All = 0,
}

impl RequestUriProtocol {
    /// Convert from u32 (for JavaScript interop)
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(RequestUriProtocol::All),
            1 => Some(RequestUriProtocol::Stam),
            2 => Some(RequestUriProtocol::Http),
            _ => None,
        }
    }

    /// Convert to u32 (for JavaScript interop)
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    /// Check if a URI matches this protocol filter
    pub fn matches(&self, uri: &str) -> bool {
        match self {
            RequestUriProtocol::All => true,
            RequestUriProtocol::Stam => uri.starts_with("stam://"),
            RequestUriProtocol::Http => uri.starts_with("http://") || uri.starts_with("https://"),
        }
    }
}

/// Request object passed to RequestUri handlers
#[derive(Debug, Clone)]
pub struct RequestUri {
    /// The complete URI being requested
    pub uri: String,
}

impl RequestUri {
    /// Create a new RequestUri
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }

    /// Get the path portion of the URI (after the authority/port)
    pub fn path(&self) -> &str {
        // Parse URI to extract path
        // Format: scheme://authority/path
        if let Some(after_scheme) = self.uri.split("://").nth(1) {
            if let Some(slash_pos) = after_scheme.find('/') {
                return &after_scheme[slash_pos..];
            }
        }
        "/"
    }
}

/// Response object for RequestUri handlers
///
/// This object is allocated by the Core and passed to handlers.
/// Handlers manipulate it through the provided API methods.
pub struct UriResponse {
    /// HTTP status code (default: 404)
    pub status: u16,
    /// Whether the request has been handled (default: false)
    pub handled: bool,
    /// Zero-copy buffer for response data
    pub buffer: Vec<u8>,
    /// Actual size of data written to buffer
    pub buffer_size: u64,
    /// Optional file path for file-based responses
    pub filepath: String,
}

impl Default for UriResponse {
    fn default() -> Self {
        Self {
            status: 404,
            handled: false,
            buffer: Vec::new(),
            buffer_size: 0,
            filepath: String::new(),
        }
    }
}

impl UriResponse {
    /// Create a new UriResponse with a specified buffer size
    pub fn new(buffer_size: usize) -> Self {
        Self {
            status: 404,
            handled: false,
            buffer: vec![0u8; buffer_size],
            buffer_size: 0,
            filepath: String::new(),
        }
    }

    /// Set the HTTP status code
    pub fn set_status(&mut self, status: u16) {
        self.status = status;
    }

    /// Set the filepath for file-based responses
    pub fn set_filepath(&mut self, path: impl Into<String>) {
        self.filepath = path.into();
    }

    /// Set the actual size of data written to buffer
    pub fn set_size(&mut self, size: u64) {
        self.buffer_size = size;
    }

    /// Set whether the request has been handled
    pub fn set_handled(&mut self, handled: bool) {
        self.handled = handled;
    }

    /// Mark the response as an error (status 500, handled=true, clear data)
    pub fn set_error(&mut self) {
        self.status = 500;
        self.handled = true;
        self.buffer_size = 0;
        self.filepath.clear();
    }
}

/// Handler registration information
#[derive(Clone)]
pub struct EventHandler {
    /// ID of the mod that registered this handler
    pub mod_id: String,
    /// Priority (lower numbers execute first)
    pub priority: i32,
    /// Protocol filter (for RequestUri)
    pub protocol: RequestUriProtocol,
    /// Route prefix filter (for RequestUri)
    pub route: String,
    /// Unique handler ID (for removal)
    pub handler_id: u64,
}

/// Event dispatcher that manages handler registration and execution
#[derive(Clone)]
pub struct EventDispatcher {
    /// Handlers organized by event key (string representation)
    /// Uses string keys to support both SystemEvents and custom events
    handlers: Arc<RwLock<HashMap<String, Vec<EventHandler>>>>,
    /// Counter for generating unique handler IDs
    next_handler_id: Arc<RwLock<u64>>,
    /// Channel sender for send_event requests (JS -> main loop)
    send_event_tx: Arc<RwLock<Option<mpsc::Sender<SendEventRequest>>>>,
    /// Channel receiver for send_event requests (main loop)
    send_event_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<SendEventRequest>>>>,
}

impl EventDispatcher {
    /// Create a new EventDispatcher
    pub fn new() -> Self {
        // Create mpsc channel for send_event requests (buffered with capacity 16)
        let (tx, rx) = mpsc::channel::<SendEventRequest>(16);

        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
            next_handler_id: Arc::new(RwLock::new(1)),
            send_event_tx: Arc::new(RwLock::new(Some(tx))),
            send_event_rx: Arc::new(tokio::sync::Mutex::new(Some(rx))),
        }
    }

    /// Register an event handler for a system event
    ///
    /// # Arguments
    /// * `event` - The system event type to handle
    /// * `mod_id` - ID of the registering mod
    /// * `priority` - Handler priority (lower executes first)
    /// * `protocol` - Protocol filter (for RequestUri)
    /// * `route` - Route prefix filter (for RequestUri)
    ///
    /// # Returns
    /// Unique handler ID for later removal
    pub fn register_handler(
        &self,
        event: SystemEvents,
        mod_id: impl Into<String>,
        priority: i32,
        protocol: RequestUriProtocol,
        route: impl Into<String>,
    ) -> u64 {
        self.register_handler_for_key(
            EventKey::System(event),
            mod_id,
            priority,
            protocol,
            route,
        )
    }

    /// Register an event handler for a custom event
    ///
    /// # Arguments
    /// * `event_name` - The custom event name
    /// * `mod_id` - ID of the registering mod
    /// * `priority` - Handler priority (lower executes first)
    ///
    /// # Returns
    /// Unique handler ID for later removal
    pub fn register_custom_handler(
        &self,
        event_name: impl Into<String>,
        mod_id: impl Into<String>,
        priority: i32,
    ) -> u64 {
        self.register_handler_for_key(
            EventKey::Custom(event_name.into()),
            mod_id,
            priority,
            RequestUriProtocol::All,
            "",
        )
    }

    /// Register an event handler for any event key
    fn register_handler_for_key(
        &self,
        event_key: EventKey,
        mod_id: impl Into<String>,
        priority: i32,
        protocol: RequestUriProtocol,
        route: impl Into<String>,
    ) -> u64 {
        let handler_id = {
            let mut id = self.next_handler_id.write().unwrap();
            let current = *id;
            *id += 1;
            current
        };

        let handler = EventHandler {
            mod_id: mod_id.into(),
            priority,
            protocol,
            route: route.into(),
            handler_id,
        };

        let key = event_key.to_string_key();
        let mut handlers = self.handlers.write().unwrap();
        let event_handlers = handlers.entry(key).or_insert_with(Vec::new);
        event_handlers.push(handler);

        // Sort by priority (lower first)
        event_handlers.sort_by_key(|h| h.priority);

        handler_id
    }

    /// Unregister a handler by its ID
    pub fn unregister_handler(&self, handler_id: u64) -> bool {
        let mut handlers = self.handlers.write().unwrap();
        for event_handlers in handlers.values_mut() {
            if let Some(pos) = event_handlers
                .iter()
                .position(|h| h.handler_id == handler_id)
            {
                event_handlers.remove(pos);
                return true;
            }
        }
        false
    }

    /// Unregister all handlers for a specific mod
    pub fn unregister_mod_handlers(&self, mod_id: &str) {
        let mut handlers = self.handlers.write().unwrap();
        for event_handlers in handlers.values_mut() {
            event_handlers.retain(|h| h.mod_id != mod_id);
        }
    }

    /// Get handlers for a specific event, filtered by request
    ///
    /// For RequestUri events, filters by protocol and route prefix.
    pub fn get_handlers_for_uri_request(&self, uri: &str) -> Vec<EventHandler> {
        let handlers = self.handlers.read().unwrap();
        let path = extract_uri_path(uri);
        let key = EventKey::System(SystemEvents::RequestUri).to_string_key();

        handlers
            .get(&key)
            .map(|event_handlers| {
                event_handlers
                    .iter()
                    .filter(|h| {
                        // Check protocol filter
                        if !h.protocol.matches(uri) {
                            return false;
                        }
                        // Check route prefix
                        if !h.route.is_empty() && !path.starts_with(&h.route) {
                            return false;
                        }
                        true
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get handlers for a custom event by name
    ///
    /// Returns all handlers registered for the given custom event name,
    /// sorted by priority.
    pub fn get_handlers_for_custom_event(&self, event_name: &str) -> Vec<EventHandler> {
        let handlers = self.handlers.read().unwrap();
        let key = EventKey::Custom(event_name.to_string()).to_string_key();

        handlers
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }

    /// Get the number of registered handlers for a system event type
    pub fn handler_count(&self, event: SystemEvents) -> usize {
        let handlers = self.handlers.read().unwrap();
        let key = EventKey::System(event).to_string_key();
        handlers.get(&key).map(|h| h.len()).unwrap_or(0)
    }

    /// Get the number of registered handlers for a custom event
    pub fn custom_handler_count(&self, event_name: &str) -> usize {
        let handlers = self.handlers.read().unwrap();
        let key = EventKey::Custom(event_name.to_string()).to_string_key();
        handlers.get(&key).map(|h| h.len()).unwrap_or(0)
    }

    /// Send a request to dispatch a custom event and wait for completion
    ///
    /// This is called by the JS binding `system.send_event(event_name, ...args)`.
    /// The request is sent to the main loop which will process it and respond.
    pub async fn request_send_event(&self, event_name: String, args: Vec<String>) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();

        let request = SendEventRequest {
            event_name,
            args,
            response_tx,
        };

        // Get the sender
        let tx = {
            let guard = self.send_event_tx.read().unwrap();
            guard.clone()
        };

        let tx = tx.ok_or_else(|| "Send event channel not available".to_string())?;

        // Send the request
        tx.send(request).await.map_err(|_| "Failed to send event request".to_string())?;

        // Wait for the response
        response_rx.await.map_err(|_| "Send event request was cancelled".to_string())?
    }

    /// Take the send_event request receiver (can only be called once)
    ///
    /// This is used by the main loop to receive and process send_event requests.
    pub async fn take_send_event_receiver(&self) -> Option<mpsc::Receiver<SendEventRequest>> {
        let mut guard = self.send_event_rx.lock().await;
        guard.take()
    }
}

impl Default for EventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the path portion from a URI
fn extract_uri_path(uri: &str) -> &str {
    // Format: scheme://authority/path
    if let Some(after_scheme) = uri.split("://").nth(1) {
        if let Some(slash_pos) = after_scheme.find('/') {
            return &after_scheme[slash_pos..];
        }
    }
    "/"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_events_conversion() {
        assert_eq!(SystemEvents::from_u32(1), Some(SystemEvents::RequestUri));
        assert_eq!(SystemEvents::from_u32(99), None);
        assert_eq!(SystemEvents::RequestUri.to_u32(), 1);
    }

    #[test]
    fn test_protocol_matching() {
        assert!(RequestUriProtocol::All.matches("stam://localhost/test"));
        assert!(RequestUriProtocol::All.matches("http://localhost/test"));

        assert!(RequestUriProtocol::Stam.matches("stam://localhost/test"));
        assert!(!RequestUriProtocol::Stam.matches("http://localhost/test"));

        assert!(!RequestUriProtocol::Http.matches("stam://localhost/test"));
        assert!(RequestUriProtocol::Http.matches("http://localhost/test"));
        assert!(RequestUriProtocol::Http.matches("https://localhost/test"));
    }

    #[test]
    fn test_uri_path_extraction() {
        assert_eq!(
            extract_uri_path("stam://localhost:9999/mods-manager/download"),
            "/mods-manager/download"
        );
        assert_eq!(extract_uri_path("http://example.com/api/v1"), "/api/v1");
        assert_eq!(extract_uri_path("stam://localhost"), "/");
    }

    #[test]
    fn test_handler_registration() {
        let dispatcher = EventDispatcher::new();

        let id1 = dispatcher.register_handler(
            SystemEvents::RequestUri,
            "mod-a",
            100,
            RequestUriProtocol::Stam,
            "/api/",
        );

        let id2 = dispatcher.register_handler(
            SystemEvents::RequestUri,
            "mod-b",
            50,
            RequestUriProtocol::All,
            "",
        );

        assert_ne!(id1, id2);
        assert_eq!(dispatcher.handler_count(SystemEvents::RequestUri), 2);

        // Handlers should be sorted by priority (50 before 100)
        let handlers = dispatcher.get_handlers_for_uri_request("stam://localhost/api/test");
        assert_eq!(handlers.len(), 2);
        assert_eq!(handlers[0].mod_id, "mod-b");
        assert_eq!(handlers[1].mod_id, "mod-a");
    }

    #[test]
    fn test_handler_filtering() {
        let dispatcher = EventDispatcher::new();

        dispatcher.register_handler(
            SystemEvents::RequestUri,
            "stam-handler",
            100,
            RequestUriProtocol::Stam,
            "",
        );

        dispatcher.register_handler(
            SystemEvents::RequestUri,
            "http-handler",
            100,
            RequestUriProtocol::Http,
            "",
        );

        // STAM request should only match stam-handler
        let handlers = dispatcher.get_handlers_for_uri_request("stam://localhost/test");
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0].mod_id, "stam-handler");

        // HTTP request should only match http-handler
        let handlers = dispatcher.get_handlers_for_uri_request("http://localhost/test");
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0].mod_id, "http-handler");
    }

    #[test]
    fn test_route_filtering() {
        let dispatcher = EventDispatcher::new();

        dispatcher.register_handler(
            SystemEvents::RequestUri,
            "api-handler",
            100,
            RequestUriProtocol::All,
            "/api/",
        );

        dispatcher.register_handler(
            SystemEvents::RequestUri,
            "catch-all",
            200,
            RequestUriProtocol::All,
            "",
        );

        // /api/ request matches both
        let handlers = dispatcher.get_handlers_for_uri_request("stam://localhost/api/test");
        assert_eq!(handlers.len(), 2);

        // /other/ request only matches catch-all
        let handlers = dispatcher.get_handlers_for_uri_request("stam://localhost/other/test");
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0].mod_id, "catch-all");
    }

    #[test]
    fn test_handler_unregistration() {
        let dispatcher = EventDispatcher::new();

        let id = dispatcher.register_handler(
            SystemEvents::RequestUri,
            "test-mod",
            100,
            RequestUriProtocol::All,
            "",
        );

        assert_eq!(dispatcher.handler_count(SystemEvents::RequestUri), 1);

        assert!(dispatcher.unregister_handler(id));
        assert_eq!(dispatcher.handler_count(SystemEvents::RequestUri), 0);

        // Second unregister should fail
        assert!(!dispatcher.unregister_handler(id));
    }

    #[test]
    fn test_mod_handler_cleanup() {
        let dispatcher = EventDispatcher::new();

        dispatcher.register_handler(
            SystemEvents::RequestUri,
            "mod-a",
            100,
            RequestUriProtocol::All,
            "",
        );

        dispatcher.register_handler(
            SystemEvents::RequestUri,
            "mod-a",
            200,
            RequestUriProtocol::All,
            "/api/",
        );

        dispatcher.register_handler(
            SystemEvents::RequestUri,
            "mod-b",
            150,
            RequestUriProtocol::All,
            "",
        );

        assert_eq!(dispatcher.handler_count(SystemEvents::RequestUri), 3);

        dispatcher.unregister_mod_handlers("mod-a");
        assert_eq!(dispatcher.handler_count(SystemEvents::RequestUri), 1);

        let handlers = dispatcher.get_handlers_for_uri_request("stam://localhost/test");
        assert_eq!(handlers[0].mod_id, "mod-b");
    }
}
