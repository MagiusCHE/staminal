use rquickjs::{Context, Runtime, Module, Object};
use rquickjs::loader::{ModuleLoader, FileResolver, Loader};
use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use tracing::{info, error, debug};

use crate::{RuntimeAdapter, ModReturnValue};
use crate::api::AppApi;
use super::{JsRuntimeConfig, bindings};

/// Custom filesystem loader for JavaScript modules
struct FilesystemLoader;

impl Loader for FilesystemLoader {
    fn load<'js>(&mut self, ctx: &rquickjs::Ctx<'js>, path: &str) -> rquickjs::Result<Module<'js>> {
        debug!("FilesystemLoader: Loading module from path: {}", path);

        // Read file content
        let content = fs::read_to_string(path)
            .map_err(|e| {
                error!("FilesystemLoader: Failed to read file '{}': {}", path, e);
                rquickjs::Error::new_loading(path)
            })?;

        debug!("FilesystemLoader: Successfully read {} bytes from {}", content.len(), path);

        // Compile and return the module
        Module::declare(ctx.clone(), path, content)
            .map_err(|e| {
                error!("FilesystemLoader: Failed to declare module '{}': {:?}", path, e);
                e
            })
    }
}

/// Represents a loaded mod with its own isolated context
struct LoadedMod {
    context: Context,
    module_path: String,
    #[allow(dead_code)]
    mod_dir: PathBuf,  // Directory containing the mod (for resolving relative imports)
}

/// JavaScript runtime adapter for QuickJS
///
/// Each mod gets its own isolated Context to prevent interference between mods.
/// All contexts share the same Runtime with a shared module loader.
pub struct JsRuntimeAdapter {
    runtime: Runtime,
    config: JsRuntimeConfig,
    /// Map of mod_id to loaded mod instance
    loaded_mods: HashMap<String, LoadedMod>,
    /// Collection of all mod directories for module resolution
    mod_dirs: Vec<PathBuf>,
}

impl JsRuntimeAdapter {
    /// Create a new JavaScript runtime adapter with QuickJS
    ///
    /// # Arguments
    /// * `config` - Runtime configuration containing game directories
    pub fn new(config: JsRuntimeConfig) -> Result<Self, Box<dyn std::error::Error>> {
        debug!("Initializing QuickJS runtime for mods");

        let runtime = Runtime::new()?;

        // Configure custom filesystem loader with resolver
        // The resolver handles path resolution, the loader reads files from disk
        let resolver = FileResolver::default()
            .with_pattern("**/*.js");  // Match .js files
        let loader = (FilesystemLoader, ModuleLoader::default());
        runtime.set_loader(resolver, loader);

        let js_runtime = Self {
            runtime,
            config,
            loaded_mods: HashMap::new(),
            mod_dirs: Vec::new(),
        };

        info!("JavaScript runtime initialized successfully");
        Ok(js_runtime)
    }

    /// Setup all global APIs in a context
    fn setup_global_apis(&self, context: &Context) -> Result<(), Box<dyn std::error::Error>> {
        let game_data_dir = self.config.game_data_dir().clone();
        let game_config_dir = self.config.game_config_dir().clone();

        context.with(|ctx| {
            // Register console API
            bindings::setup_console_api(ctx.clone())?;

            // Register process API with game-specific directories
            let app_api = AppApi::new(game_data_dir, game_config_dir);
            bindings::setup_process_api(ctx.clone(), app_api)?;

            // Future: Allow dynamic API registration via ApiRegistry
            // This would enable client/server to register different APIs

            Ok::<(), rquickjs::Error>(())
        })?;

        Ok(())
    }

    /// Format a JavaScript error with stack trace in Node.js style
    fn format_js_error(ctx: &rquickjs::Ctx, _error: &rquickjs::Error) -> String {
        use rquickjs::Value;

        let mut output = String::new();

        // Get the exception value
        let exception = ctx.catch();

        // Try to convert to object to access properties
        if let Some(obj) = exception.as_object() {
            let mut error_name = String::from("Error");
            let mut error_message = String::new();
            let mut stack_trace = String::new();

            // Try to get error name (e.g., "TypeError", "ReferenceError")
            if let Ok(name_prop) = obj.get::<_, Value>("name") {
                if let Some(name_str) = name_prop.as_string() {
                    if let Ok(name) = name_str.to_string() {
                        error_name = name;
                    }
                }
            }

            // Try to get error message
            if let Ok(msg_prop) = obj.get::<_, Value>("message") {
                if let Some(msg_str) = msg_prop.as_string() {
                    if let Ok(msg) = msg_str.to_string() {
                        error_message = msg;
                    }
                }
            }

            // Try to get stack trace
            if let Ok(stack_prop) = obj.get::<_, Value>("stack") {
                if let Some(stack_str) = stack_prop.as_string() {
                    if let Ok(stack) = stack_str.to_string() {
                        stack_trace = stack;
                    }
                }
            }

            // Format output: "Error: message\n    at ..."
            if !error_message.is_empty() {
                output.push_str(&format!("{}: {}", error_name, error_message));
            } else {
                output.push_str(&error_name);
            }

            // Add stack trace if available
            if !stack_trace.is_empty() {
                // Check if stack already contains the error message
                // QuickJS sometimes includes it, sometimes doesn't
                if !stack_trace.starts_with(&error_name) {
                    output.push('\n');
                    output.push_str(&stack_trace);
                } else {
                    // Stack already has the error message, just use it
                    output = stack_trace;
                }
            }

            return output;
        }

        // Fallback: try to convert exception directly to string
        if let Some(s) = exception.as_string() {
            if let Ok(msg) = s.to_string() {
                output.push_str(&format!("Error: {}", msg));
                return output;
            }
        }

        output.push_str("Error: Unknown JavaScript error");
        output
    }
}

impl RuntimeAdapter for JsRuntimeAdapter {
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Loading JavaScript module: {} from {}", mod_id, mod_path.display());

        // Get the mod directory (parent of the entry point file)
        let mod_dir = mod_path.parent()
            .ok_or_else(|| format!("Cannot determine mod directory for '{}'", mod_path.display()))?
            .to_path_buf();

        // Add mod directory to the list of search paths and update loader
        if !self.mod_dirs.contains(&mod_dir) {
            // Add to the list first
            self.mod_dirs.push(mod_dir.clone());

            // Update the loader with all mod directories
            let mut resolver = FileResolver::default()
                .with_pattern("**/*.js");  // Match .js files

            for dir in &self.mod_dirs {
                let dir_str = dir.to_str()
                    .ok_or_else(|| format!("Mod directory path contains invalid UTF-8: {}", dir.display()))?;
                resolver = resolver.with_path(dir_str);
            }

            let loader = (FilesystemLoader, ModuleLoader::default());
            self.runtime.set_loader(resolver, loader);
        }

        // Create a new isolated Context for this mod
        let context = Context::full(&self.runtime)?;

        // Setup global APIs for this mod's context
        self.setup_global_apis(&context)?;

        // Set global __MOD_ID__ variable for console logging
        context.with(|ctx| {
            ctx.globals().set("__MOD_ID__", mod_id)?;
            Ok::<(), rquickjs::Error>(())
        })?;

        // Get the filename relative to mod directory for import
        let filename = mod_path.file_name()
            .ok_or_else(|| format!("Cannot determine filename for '{}'", mod_path.display()))?
            .to_str()
            .ok_or_else(|| format!("Filename contains invalid UTF-8: {}", mod_path.display()))?;

        // Load the module from the filesystem using import()
        // Use filename relative to mod directory (which is in the resolver path)
        context.with(|ctx| {
            debug!("Importing module '{}' for mod '{}'", filename, mod_id);

            match Module::import(&ctx, filename) {
                Ok(promise) => {
                    // Wait for the module to finish loading
                    match promise.finish::<()>() {
                        Ok(_) => {
                            info!("Mod '{}' loaded successfully", mod_id);
                            Ok::<(), Box<dyn std::error::Error>>(())
                        }
                        Err(e) => {
                            let error_msg = Self::format_js_error(&ctx, &e);
                            error!("\n{}", error_msg);
                            Err(format!("JavaScript error in mod '{}'", mod_id).into())
                        }
                    }
                }
                Err(e) => {
                    let error_msg = Self::format_js_error(&ctx, &e);
                    error!("\n{}", error_msg);
                    Err(format!("JavaScript error in mod '{}'", mod_id).into())
                }
            }
        })?;

        // Store the loaded mod with the relative filename (not absolute path)
        // This is important because we need to use the same import path that the resolver understands
        self.loaded_mods.insert(mod_id.to_string(), LoadedMod {
            context,
            module_path: filename.to_string(),  // Use relative filename, not absolute path
            mod_dir,
        });

        Ok(())
    }

    fn call_mod_function(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Calling JavaScript function '{}' for mod '{}'", function_name, mod_id);

        // Get the loaded mod
        let loaded_mod = self.loaded_mods.get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        // Access the module namespace and call the exported function
        loaded_mod.context.with(|ctx| {
            let module_path = &loaded_mod.module_path;

            // Import the module (uses cached version if already loaded)
            match Module::import(&ctx, module_path.clone()) {
                Ok(promise) => {
                    // Wait for import to complete and get the module namespace
                    match promise.finish::<Object>() {
                        Ok(module_namespace) => {
                            // Try to get the exported function from the namespace
                            match module_namespace.get::<_, rquickjs::Function>(function_name) {
                                Ok(func) => {
                                    // Call the function
                                    match func.call::<(), ()>(()) {
                                        Ok(_) => {
                                            debug!("Function '{}' executed successfully for mod '{}'", function_name, mod_id);
                                            Ok(())
                                        }
                                        Err(e) => {
                                            let error_msg = Self::format_js_error(&ctx, &e);
                                            error!("\n{}", error_msg);
                                            Err(format!("JavaScript error in '{}' for mod '{}'", function_name, mod_id).into())
                                        }
                                    }
                                }
                                Err(_) => {
                                    // Function might not exist (optional)
                                    debug!("Function '{}' not found or not exported for mod '{}'", function_name, mod_id);
                                    Ok(())
                                }
                            }
                        }
                        Err(e) => {
                            let error_msg = Self::format_js_error(&ctx, &e);
                            error!("\n{}", error_msg);
                            Err(format!("Failed to get module namespace for mod '{}'", mod_id).into())
                        }
                    }
                }
                Err(e) => {
                    let error_msg = Self::format_js_error(&ctx, &e);
                    error!("\n{}", error_msg);
                    Err(format!("Failed to import module for mod '{}'", mod_id).into())
                }
            }
        })
    }

    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        debug!("Calling JavaScript function '{}' for mod '{}' with return", function_name, mod_id);

        // Get the loaded mod
        let loaded_mod = self.loaded_mods.get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        // Access the module namespace and call the exported function
        loaded_mod.context.with(|ctx| {
            let module_path = &loaded_mod.module_path;

            // Import the module (uses cached version if already loaded)
            match Module::import(&ctx, module_path.clone()) {
                Ok(promise) => {
                    // Wait for import to complete and get the module namespace
                    match promise.finish::<Object>() {
                        Ok(module_namespace) => {
                            // Try to get the exported function from the namespace
                            match module_namespace.get::<_, rquickjs::Function>(function_name) {
                                Ok(func) => {
                                    // Try to call and get return value as String first
                                    // In the future, we could detect the return type dynamically
                                    match func.call::<(), String>(()) {
                                        Ok(value) => {
                                            debug!("Function '{}' returned string for mod '{}'", function_name, mod_id);
                                            Ok(ModReturnValue::String(value))
                                        }
                                        Err(e) => {
                                            let error_msg = Self::format_js_error(&ctx, &e);
                                            error!("\n{}", error_msg);
                                            Err(format!("JavaScript error in '{}' for mod '{}'", function_name, mod_id).into())
                                        }
                                    }
                                }
                                Err(_) => {
                                    // Function might not exist (optional)
                                    debug!("Function '{}' not found or not exported for mod '{}'", function_name, mod_id);
                                    Ok(ModReturnValue::None)
                                }
                            }
                        }
                        Err(e) => {
                            let error_msg = Self::format_js_error(&ctx, &e);
                            error!("\n{}", error_msg);
                            Err(format!("Failed to get module namespace for mod '{}'", mod_id).into())
                        }
                    }
                }
                Err(e) => {
                    let error_msg = Self::format_js_error(&ctx, &e);
                    error!("\n{}", error_msg);
                    Err(format!("Failed to import module for mod '{}'", mod_id).into())
                }
            }
        })
    }
}
