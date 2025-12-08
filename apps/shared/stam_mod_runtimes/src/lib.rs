//! Stam Runtime System
//!
//! Provides a unified, modular runtime system for executing mods in different languages
//! (JavaScript, Lua, C#, Rust, C++). This crate is shared between client and server.
//!
//! # Architecture
//!
//! - **RuntimeAdapter**: Trait that all runtime implementations must implement
//! - **ApiProvider**: Trait for APIs that can be injected into runtimes (console, process, etc.)
//! - **RuntimeManager**: Manages multiple runtimes and dispatches calls to the appropriate one
//! - **ApiRegistry**: Registry for configuring which APIs are available to mods

use std::collections::HashMap;
use std::path::Path;

pub mod api;
pub mod runtime_type;
pub mod terminal_input;

// Re-export stam_log for convenience
pub use stam_log as logging;

// Conditional module imports based on features
#[cfg(feature = "js")]
pub mod adapters;

pub use runtime_type::RuntimeType;

// Re-export AsyncRuntime type for event loop integration
#[cfg(feature = "js")]
pub use rquickjs::AsyncRuntime as JsAsyncRuntime;

/// Return value from a mod function call
#[derive(Debug, Clone)]
pub enum ModReturnValue {
    None,
    String(String),
    Bool(bool),
    Int(i32),
    // Future: Object(HashMap<String, ModReturnValue>), Array(Vec<ModReturnValue>)
}

/// Trait that all runtime adapters must implement
///
/// A runtime adapter wraps a specific scripting language runtime (QuickJS, Lua VM, etc.)
/// and provides a uniform interface for loading mods and calling their functions.
///
/// Note: RuntimeAdapter does not require Send because some runtimes (like QuickJS)
/// are single-threaded and their contexts cannot be sent across threads.
pub trait RuntimeAdapter {
    /// Load a mod script into this runtime
    ///
    /// # Arguments
    /// * `mod_path` - Path to the mod's entry point file
    /// * `mod_id` - Unique identifier for the mod
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str) -> Result<(), Box<dyn std::error::Error>>;

    /// Call a function in a mod without return value
    ///
    /// # Arguments
    /// * `mod_id` - ID of the mod
    /// * `function_name` - Name of the function to call (e.g., "onAttach", "onBootstrap")
    fn call_mod_function(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn std::error::Error>>;

    /// Call a function in a mod with a return value
    ///
    /// # Arguments
    /// * `mod_id` - ID of the mod
    /// * `function_name` - Name of the function to call
    ///
    /// # Returns
    /// A `ModReturnValue` which can be pattern matched to extract the actual value
    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>>;

    /// Call an event handler by its handler ID
    ///
    /// This is used to invoke handlers registered via `system.register_custom_event`.
    /// The handler function was stored in the runtime's context with the handler_id as key.
    ///
    /// # Arguments
    /// * `handler_id` - The unique handler ID returned from registration
    /// * `event_name` - The name of the event being dispatched
    /// * `args` - JSON-serialized arguments to pass to the handler
    ///
    /// # Returns
    /// Ok(()) if the handler was called successfully, Err otherwise
    fn call_event_handler(
        &mut self,
        handler_id: u64,
        event_name: &str,
        args: &[String],
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Dispatch a TerminalKeyPressed event to all registered handlers
    ///
    /// This method finds all handlers registered for TerminalKeyPressed, calls them
    /// in priority order (lowest first), and returns whether the event was handled.
    ///
    /// # Arguments
    /// * `request` - The terminal key request containing key and modifier information
    ///
    /// # Returns
    /// A `TerminalKeyResponse` containing whether the event was handled
    fn dispatch_terminal_key(&self, request: &api::TerminalKeyRequest) -> api::TerminalKeyResponse;

    /// Get the number of handlers registered for TerminalKeyPressed event
    ///
    /// This is used to determine if any mod has registered to handle terminal input,
    /// which affects whether the default "Ctrl+C to exit" message should be shown.
    fn terminal_key_handler_count(&self) -> usize;

    /// Dispatch a GraphicEngineReady event to all registered handlers
    ///
    /// This method finds all handlers registered for GraphicEngineReady, calls them
    /// in priority order (lowest first), and returns whether the event was handled.
    /// This event is client-only and is triggered when the graphic engine has been
    /// initialized and is ready to receive commands.
    ///
    /// # Arguments
    /// * `request` - The graphic engine ready request (currently empty but extensible)
    ///
    /// # Returns
    /// A `GraphicEngineReadyResponse` containing whether the event was handled
    fn dispatch_graphic_engine_ready(&self, request: &api::GraphicEngineReadyRequest) -> api::GraphicEngineReadyResponse;

    /// Dispatch a GraphicEngineWindowClosed event to all registered handlers
    ///
    /// This method finds all handlers registered for GraphicEngineWindowClosed, calls them
    /// in priority order (lowest first), and returns whether the event was handled.
    /// This event is client-only and is triggered when a window managed by the
    /// graphic engine is closed.
    ///
    /// # Arguments
    /// * `request` - The window closed request containing the window_id
    ///
    /// # Returns
    /// A `GraphicEngineWindowClosedResponse` containing whether the event was handled
    fn dispatch_graphic_engine_window_closed(&self, request: &api::GraphicEngineWindowClosedRequest) -> api::GraphicEngineWindowClosedResponse;

    /// Dispatch a custom event to all registered handlers
    ///
    /// This method finds all handlers registered for the custom event, calls them
    /// in priority order (lowest first), and returns the aggregated response.
    /// Each handler receives a request object with `args` array and a response object
    /// with `handled` flag and custom properties.
    ///
    /// **IMPORTANT**: Handler response values must be set SYNCHRONOUSLY before any
    /// `await` points. Values set after an `await` will not be captured.
    ///
    /// # Arguments
    /// * `request` - The custom event request containing event_name and args
    ///
    /// # Returns
    /// A `CustomEventResponse` containing whether the event was handled and any results
    fn dispatch_custom_event(&self, request: &api::CustomEventRequest) -> api::CustomEventResponse;

    // Note: dispatch_widget_event has been removed. Use ECS entity event callbacks instead.

    /// Dispatch a direct entity event callback (client-only)
    ///
    /// This is called when an ECS entity with a registered event callback is triggered.
    /// Unlike global events, this uses a direct callback mechanism where the callback
    /// function is stored in the runtime's registry and invoked directly by entity ID.
    ///
    /// This is language-agnostic: each runtime (JS, Lua, C#) maintains its own callback
    /// registry and invokes callbacks in its native way.
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID that triggered the event
    /// * `event_type` - The event type (e.g., "click", "hover", "enter", "leave")
    /// * `event_data` - Event-specific data as JSON object
    ///
    /// # Returns
    /// Ok(true) if a callback was found and invoked, Ok(false) if no callback registered,
    /// Err if invocation failed
    fn dispatch_entity_event_callback(
        &self,
        _entity_id: u64,
        _event_type: &str,
        _event_data: serde_json::Value,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Default: no callback registered
        Ok(false)
    }

    /// Dispatch a window event callback
    ///
    /// This is called when a window event occurs (resize, focus, key press, etc.).
    /// Unlike global events, this uses a direct callback mechanism where the callback
    /// function is stored on the Window object via `window.onClose = ...` etc.
    ///
    /// # Arguments
    /// * `window_id` - The window ID that triggered the event
    /// * `event_type` - The event type (e.g., "close", "resize", "keyPressed", etc.)
    /// * `event_data` - Event-specific data as JSON object
    ///
    /// # Returns
    /// Ok(true) if a callback was found and invoked, Ok(false) if no callback registered,
    /// Err if invocation failed
    fn dispatch_window_event_callback(
        &self,
        _window_id: u64,
        _event_type: &str,
        _event_data: serde_json::Value,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Default: no callback registered
        Ok(false)
    }
}

/// Manager for all mod runtimes
///
/// This manages multiple runtime instances (one per runtime type) and dispatches
/// mod function calls to the appropriate runtime based on the mod's type.
pub struct RuntimeManager {
    /// Map of runtime type to adapter instance
    runtimes: HashMap<RuntimeType, Box<dyn RuntimeAdapter>>,

    /// Map of mod_id to runtime type
    mod_to_runtime: HashMap<String, RuntimeType>,
}

impl RuntimeManager {
    /// Create a new runtime manager
    pub fn new() -> Self {
        Self {
            runtimes: HashMap::new(),
            mod_to_runtime: HashMap::new(),
        }
    }

    /// Register a runtime adapter for a specific runtime type
    ///
    /// # Arguments
    /// * `runtime_type` - The type of runtime (JavaScript, Lua, etc.)
    /// * `adapter` - The adapter instance implementing RuntimeAdapter
    pub fn register_adapter(&mut self, runtime_type: RuntimeType, adapter: Box<dyn RuntimeAdapter>) {
        self.runtimes.insert(runtime_type, adapter);
    }

    /// Load a mod into the appropriate runtime based on its entry_point extension
    ///
    /// # Arguments
    /// * `mod_id` - Unique identifier for the mod
    /// * `entry_point` - Path to the mod's entry point file
    ///
    /// The runtime type is determined by the file extension:
    /// - .js -> JavaScript
    /// - .lua -> Lua (future)
    /// - .cs -> C# (future)
    /// - .rs -> Rust (future)
    /// - .cpp -> C++ (future)
    pub fn load_mod(&mut self, mod_id: &str, entry_point: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Determine runtime type from file extension
        let runtime_type = RuntimeType::from_extension(entry_point)?;

        // Get the runtime for this type
        let runtime = self.runtimes.get_mut(&runtime_type)
            .ok_or_else(|| format!("Runtime not initialized for type: {:?}", runtime_type))?;

        // Load the mod
        runtime.load_mod(entry_point, mod_id)?;

        // Register mod -> runtime mapping
        self.mod_to_runtime.insert(mod_id.to_string(), runtime_type);

        Ok(())
    }

    /// Call a function in a mod without expecting a return value
    ///
    /// This abstracts away the runtime type - the caller doesn't need to know
    /// whether the mod uses JavaScript, Lua, or any other runtime.
    ///
    /// # Arguments
    /// * `mod_id` - ID of the mod
    /// * `function_name` - Name of the function to call (e.g., "onAttach", "onBootstrap")
    pub fn call_mod_function(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Look up which runtime this mod uses
        let runtime_type = self.mod_to_runtime.get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        // Get the runtime adapter
        let runtime = self.runtimes.get_mut(runtime_type)
            .ok_or_else(|| format!("Runtime {:?} not available", runtime_type))?;

        // Call the function
        runtime.call_mod_function(mod_id, function_name)
    }

    /// Call a function in a mod and get a return value
    ///
    /// # Arguments
    /// * `mod_id` - ID of the mod
    /// * `function_name` - Name of the function to call
    ///
    /// # Returns
    /// A `ModReturnValue` which can be pattern matched to extract the actual value
    pub fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        // Look up which runtime this mod uses
        let runtime_type = self.mod_to_runtime.get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        // Get the runtime adapter
        let runtime = self.runtimes.get_mut(runtime_type)
            .ok_or_else(|| format!("Runtime {:?} not available", runtime_type))?;

        // Call the function
        runtime.call_mod_function_with_return(mod_id, function_name)
    }

    /// Get the runtime type for a loaded mod
    pub fn get_mod_runtime_type(&self, mod_id: &str) -> Option<RuntimeType> {
        self.mod_to_runtime.get(mod_id).copied()
    }

    /// Get list of all loaded mods
    pub fn loaded_mods(&self) -> Vec<&str> {
        self.mod_to_runtime.keys().map(|s| s.as_str()).collect()
    }

    /// Call an event handler by its handler ID
    ///
    /// This delegates to the appropriate runtime adapter. Currently assumes JavaScript
    /// runtime since that's where event handlers are registered.
    ///
    /// # Arguments
    /// * `handler_id` - The unique handler ID returned from registration
    /// * `event_name` - The name of the event being dispatched
    /// * `args` - JSON-serialized arguments to pass to the handler
    pub fn call_event_handler(
        &mut self,
        handler_id: u64,
        event_name: &str,
        args: &[String],
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Event handlers are currently only supported in JavaScript runtime
        // Get the JavaScript runtime adapter
        let runtime = self.runtimes.get_mut(&RuntimeType::JavaScript)
            .ok_or_else(|| "JavaScript runtime not available for event handlers")?;

        runtime.call_event_handler(handler_id, event_name, args)
    }

    /// Dispatch a TerminalKeyPressed event to all registered handlers
    ///
    /// This method iterates through all runtime adapters and dispatches the
    /// TerminalKeyPressed event. If any handler marks the event as handled,
    /// the loop stops and returns immediately.
    ///
    /// # Arguments
    /// * `request` - The terminal key request containing key and modifier information
    ///
    /// # Returns
    /// A `TerminalKeyResponse` containing whether the event was handled by any runtime
    pub fn dispatch_terminal_key(&self, request: &api::TerminalKeyRequest) -> api::TerminalKeyResponse {
        // Dispatch to all runtimes (currently only JavaScript)
        // If any runtime handles the event, stop and return
        for runtime in self.runtimes.values() {
            let response = runtime.dispatch_terminal_key(request);
            if response.handled {
                return response;
            }
        }
        // No runtime handled the event
        api::TerminalKeyResponse::default()
    }

    /// Get the total number of handlers registered for TerminalKeyPressed across all runtimes
    ///
    /// This is used to determine if any mod has registered to handle terminal input,
    /// which affects whether the default "Ctrl+C to exit" message should be shown.
    pub fn terminal_key_handler_count(&self) -> usize {
        self.runtimes.values().map(|r| r.terminal_key_handler_count()).sum()
    }

    /// Dispatch a GraphicEngineReady event to all registered handlers
    ///
    /// This method iterates through all runtime adapters and dispatches the
    /// GraphicEngineReady event. If any handler marks the event as handled,
    /// the loop stops and returns immediately.
    ///
    /// # Arguments
    /// * `request` - The graphic engine ready request (currently empty but extensible)
    ///
    /// # Returns
    /// A `GraphicEngineReadyResponse` containing whether the event was handled by any runtime
    pub fn dispatch_graphic_engine_ready(&self, request: &api::GraphicEngineReadyRequest) -> api::GraphicEngineReadyResponse {
        // Dispatch to all runtimes (currently only JavaScript)
        // If any runtime handles the event, stop and return
        for runtime in self.runtimes.values() {
            let response = runtime.dispatch_graphic_engine_ready(request);
            if response.handled {
                return response;
            }
        }
        // No runtime handled the event
        api::GraphicEngineReadyResponse::default()
    }

    /// Dispatch a GraphicEngineWindowClosed event to all registered handlers
    ///
    /// This method iterates through all runtime adapters and dispatches the
    /// GraphicEngineWindowClosed event. If any handler marks the event as handled,
    /// the loop stops and returns immediately.
    ///
    /// # Arguments
    /// * `request` - The window closed request containing the window_id
    ///
    /// # Returns
    /// A `GraphicEngineWindowClosedResponse` containing whether the event was handled by any runtime
    pub fn dispatch_graphic_engine_window_closed(&self, request: &api::GraphicEngineWindowClosedRequest) -> api::GraphicEngineWindowClosedResponse {
        // Dispatch to all runtimes (currently only JavaScript)
        // If any runtime handles the event, stop and return
        for runtime in self.runtimes.values() {
            let response = runtime.dispatch_graphic_engine_window_closed(request);
            if response.handled {
                return response;
            }
        }
        // No runtime handled the event
        api::GraphicEngineWindowClosedResponse::default()
    }

    /// Dispatch a custom event to all registered handlers
    ///
    /// This method iterates through all runtime adapters and dispatches the
    /// custom event. Unlike other events, custom events aggregate results from
    /// all handlers (handled flag and results array are combined).
    ///
    /// # Arguments
    /// * `request` - The custom event request containing event_name and args
    ///
    /// # Returns
    /// A `CustomEventResponse` containing whether the event was handled and any results
    pub fn dispatch_custom_event(&self, request: &api::CustomEventRequest) -> api::CustomEventResponse {
        let mut aggregated = api::CustomEventResponse::default();

        // Dispatch to all runtimes (currently only JavaScript)
        // Aggregate properties from all handlers
        for runtime in self.runtimes.values() {
            let response = runtime.dispatch_custom_event(request);
            if response.handled {
                aggregated.handled = true;
            }
            // Merge properties from this runtime into aggregated response
            for (key, value) in response.properties {
                aggregated.properties.insert(key, value);
            }
        }

        aggregated
    }

    // Note: dispatch_widget_event has been removed. Use ECS entity event callbacks instead.

    /// Dispatch a direct entity event callback
    ///
    /// This is called when an ECS entity with a registered event callback is triggered.
    /// The callback is invoked directly without going through the global event system.
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID that triggered the event
    /// * `event_type` - The event type (e.g., "click", "hover", "enter", "leave")
    /// * `event_data` - Event-specific data as JSON object
    ///
    /// # Returns
    /// Ok(true) if a callback was found and invoked by any runtime, Ok(false) if none
    pub fn dispatch_entity_event_callback(
        &self,
        entity_id: u64,
        event_type: &str,
        event_data: serde_json::Value,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Dispatch to all runtimes - the one that registered the callback will handle it
        for runtime in self.runtimes.values() {
            if runtime.dispatch_entity_event_callback(entity_id, event_type, event_data.clone())? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Dispatch a direct window event callback
    ///
    /// This is called when a window event occurs (resize, focus, key press, etc.).
    /// The callback is invoked directly without going through the global event system.
    ///
    /// # Arguments
    /// * `window_id` - The window ID that triggered the event
    /// * `event_type` - The event type (e.g., "close", "resize", "keyPressed", etc.)
    /// * `event_data` - Event-specific data as JSON object
    ///
    /// # Returns
    /// Ok(true) if a callback was found and invoked by any runtime, Ok(false) if none
    pub fn dispatch_window_event_callback(
        &self,
        window_id: u64,
        event_type: &str,
        event_data: serde_json::Value,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Dispatch to all runtimes - the one that registered the callback will handle it
        for runtime in self.runtimes.values() {
            if runtime.dispatch_window_event_callback(window_id, event_type, event_data.clone())? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}
