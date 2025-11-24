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

use std::path::Path;
use std::collections::HashMap;

pub mod runtime_type;
pub mod api;

// Conditional module imports based on features
#[cfg(feature = "js")]
pub mod adapters;

pub use runtime_type::RuntimeType;

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
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}
