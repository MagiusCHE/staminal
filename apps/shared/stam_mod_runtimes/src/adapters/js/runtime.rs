use rquickjs::{AsyncContext, AsyncRuntime, Module, Object};
use rquickjs::loader::{ModuleLoader, FileResolver, Loader};
use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use std::sync::Arc;
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
    context: AsyncContext,
    module_path: String,
    #[allow(dead_code)]
    mod_dir: PathBuf,
}

/// JavaScript runtime adapter for QuickJS with async support
///
/// Each mod gets its own isolated Context to prevent interference between mods.
/// All contexts share the same Runtime with a shared module loader.
pub struct JsRuntimeAdapter {
    runtime: Arc<AsyncRuntime>,
    config: JsRuntimeConfig,
    /// Map of mod_id to loaded mod instance
    loaded_mods: HashMap<String, LoadedMod>,
    /// Collection of all mod directories for module resolution
    mod_dirs: Vec<PathBuf>,
}

impl JsRuntimeAdapter {
    /// Create a new JavaScript runtime adapter with QuickJS async support
    ///
    /// # Arguments
    /// * `config` - Runtime configuration containing game directories
    pub fn new(config: JsRuntimeConfig) -> Result<Self, Box<dyn std::error::Error>> {
        debug!("Initializing QuickJS async runtime for mods");

        let runtime = AsyncRuntime::new()?;

        let js_runtime = Self {
            runtime: Arc::new(runtime),
            config,
            loaded_mods: HashMap::new(),
            mod_dirs: Vec::new(),
        };

        info!("JavaScript async runtime initialized successfully");
        Ok(js_runtime)
    }

    /// Get a clone of the async runtime for the event loop
    pub fn get_runtime(&self) -> Arc<AsyncRuntime> {
        Arc::clone(&self.runtime)
    }

    /// Setup all global APIs in a context
    async fn setup_global_apis(&self, context: &AsyncContext) -> Result<(), Box<dyn std::error::Error>> {
        let game_data_dir = self.config.game_data_dir().clone();
        let game_config_dir = self.config.game_config_dir().clone();

        context.with(|ctx| {
            // Register console API
            bindings::setup_console_api(ctx.clone())?;

            // Register process API with game-specific directories
            let app_api = AppApi::new(game_data_dir, game_config_dir);
            bindings::setup_process_api(ctx.clone(), app_api)?;

            // Register timer API (setTimeout, setInterval, etc.)
            bindings::setup_timer_api(ctx.clone())?;

            Ok::<(), rquickjs::Error>(())
        }).await?;

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
                if !stack_trace.starts_with(&error_name) {
                    output.push('\n');
                    output.push_str(&stack_trace);
                } else {
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

    /// Load a mod asynchronously
    pub async fn load_mod_async(&mut self, mod_path: &Path, mod_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Loading JavaScript module: {} from {}", mod_id, mod_path.display());

        // Get the mod directory (parent of the entry point file)
        let mod_dir = mod_path.parent()
            .ok_or_else(|| format!("Cannot determine mod directory for '{}'", mod_path.display()))?
            .to_path_buf();

        // Add mod directory to the list of search paths and update loader
        if !self.mod_dirs.contains(&mod_dir) {
            self.mod_dirs.push(mod_dir.clone());

            let mut resolver = FileResolver::default()
                .with_pattern("**/*.js");

            for dir in &self.mod_dirs {
                let dir_str = dir.to_str()
                    .ok_or_else(|| format!("Mod directory path contains invalid UTF-8: {}", dir.display()))?;
                resolver = resolver.with_path(dir_str);
            }

            let loader = (FilesystemLoader, ModuleLoader::default());
            self.runtime.set_loader(resolver, loader).await;
        }

        // Create a new isolated AsyncContext for this mod
        let context = AsyncContext::full(&self.runtime).await?;

        // Setup global APIs for this mod's context
        self.setup_global_apis(&context).await?;

        // Set global __MOD_ID__ variable for console logging
        context.with(|ctx| {
            ctx.globals().set("__MOD_ID__", mod_id)?;
            Ok::<(), rquickjs::Error>(())
        }).await?;

        // Get the filename relative to mod directory for import
        let filename = mod_path.file_name()
            .ok_or_else(|| format!("Cannot determine filename for '{}'", mod_path.display()))?
            .to_str()
            .ok_or_else(|| format!("Filename contains invalid UTF-8: {}", mod_path.display()))?
            .to_string();

        let mod_id_owned = mod_id.to_string();

        // Load the module from the filesystem
        // Use Result<String, String> for ParallelSend compatibility
        let result: Result<String, String> = context.with(|ctx| {
            debug!("Importing module '{}' for mod '{}'", filename, mod_id_owned);

            match Module::import(&ctx, filename.clone()) {
                Ok(promise) => {
                    match promise.finish::<()>() {
                        Ok(_) => {
                            info!("Mod '{}' loaded successfully", mod_id_owned);
                            Ok(filename.clone())
                        }
                        Err(e) => {
                            let error_msg = Self::format_js_error(&ctx, &e);
                            error!("\n{}", error_msg);
                            Err(format!("JavaScript error in mod '{}'", mod_id_owned))
                        }
                    }
                }
                Err(e) => {
                    let error_msg = Self::format_js_error(&ctx, &e);
                    error!("\n{}", error_msg);
                    Err(format!("JavaScript error in mod '{}'", mod_id_owned))
                }
            }
        }).await;

        result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

        // Store the loaded mod
        self.loaded_mods.insert(mod_id.to_string(), LoadedMod {
            context,
            module_path: filename.to_string(),
            mod_dir,
        });

        Ok(())
    }

    /// Call a mod function asynchronously
    pub async fn call_mod_function_async(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Calling JavaScript function '{}' for mod '{}'", function_name, mod_id);

        let loaded_mod = self.loaded_mods.get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        let module_path = loaded_mod.module_path.clone();
        let mod_id_owned = mod_id.to_string();
        let function_name_owned = function_name.to_string();

        // Use Result<(), String> for ParallelSend compatibility
        let result: Result<(), String> = loaded_mod.context.with(|ctx| {
            match Module::import(&ctx, module_path.clone()) {
                Ok(promise) => {
                    match promise.finish::<Object>() {
                        Ok(module_namespace) => {
                            match module_namespace.get::<_, rquickjs::Function>(&function_name_owned) {
                                Ok(func) => {
                                    match func.call::<(), ()>(()) {
                                        Ok(_) => {
                                            debug!("Function '{}' executed successfully for mod '{}'", function_name_owned, mod_id_owned);
                                            Ok(())
                                        }
                                        Err(e) => {
                                            let error_msg = Self::format_js_error(&ctx, &e);
                                            error!("\n{}", error_msg);
                                            Err(format!("JavaScript error in '{}' for mod '{}'", function_name_owned, mod_id_owned))
                                        }
                                    }
                                }
                                Err(_) => {
                                    debug!("Function '{}' not found or not exported for mod '{}'", function_name_owned, mod_id_owned);
                                    Ok(())
                                }
                            }
                        }
                        Err(e) => {
                            let error_msg = Self::format_js_error(&ctx, &e);
                            error!("\n{}", error_msg);
                            Err(format!("Failed to get module namespace for mod '{}'", mod_id_owned))
                        }
                    }
                }
                Err(e) => {
                    let error_msg = Self::format_js_error(&ctx, &e);
                    error!("\n{}", error_msg);
                    Err(format!("Failed to import module for mod '{}'", mod_id_owned))
                }
            }
        }).await;

        result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    }

    /// Call a mod function asynchronously with return value
    pub async fn call_mod_function_with_return_async(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        debug!("Calling JavaScript function '{}' for mod '{}' with return", function_name, mod_id);

        let loaded_mod = self.loaded_mods.get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        let module_path = loaded_mod.module_path.clone();
        let mod_id_owned = mod_id.to_string();
        let function_name_owned = function_name.to_string();

        // Use Result<ModReturnValue, String> for ParallelSend compatibility
        let result: Result<ModReturnValue, String> = loaded_mod.context.with(|ctx| {
            match Module::import(&ctx, module_path.clone()) {
                Ok(promise) => {
                    match promise.finish::<Object>() {
                        Ok(module_namespace) => {
                            match module_namespace.get::<_, rquickjs::Function>(&function_name_owned) {
                                Ok(func) => {
                                    match func.call::<(), String>(()) {
                                        Ok(value) => {
                                            debug!("Function '{}' returned string for mod '{}'", function_name_owned, mod_id_owned);
                                            Ok(ModReturnValue::String(value))
                                        }
                                        Err(e) => {
                                            let error_msg = Self::format_js_error(&ctx, &e);
                                            error!("\n{}", error_msg);
                                            Err(format!("JavaScript error in '{}' for mod '{}'", function_name_owned, mod_id_owned))
                                        }
                                    }
                                }
                                Err(_) => {
                                    debug!("Function '{}' not found or not exported for mod '{}'", function_name_owned, mod_id_owned);
                                    Ok(ModReturnValue::None)
                                }
                            }
                        }
                        Err(e) => {
                            let error_msg = Self::format_js_error(&ctx, &e);
                            error!("\n{}", error_msg);
                            Err(format!("Failed to get module namespace for mod '{}'", mod_id_owned))
                        }
                    }
                }
                Err(e) => {
                    let error_msg = Self::format_js_error(&ctx, &e);
                    error!("\n{}", error_msg);
                    Err(format!("Failed to import module for mod '{}'", mod_id_owned))
                }
            }
        }).await;

        result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    }
}

// Synchronous trait implementation that wraps async calls
impl RuntimeAdapter for JsRuntimeAdapter {
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.load_mod_async(mod_path, mod_id))
        })
    }

    fn call_mod_function(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.call_mod_function_async(mod_id, function_name))
        })
    }

    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.call_mod_function_with_return_async(mod_id, function_name))
        })
    }
}

/// Run the JavaScript event loop
///
/// This function should be spawned as a task and run concurrently with other tasks.
/// It processes pending JavaScript jobs (Promises, timers spawned via ctx.spawn(), etc.)
///
/// The event loop will run until cancelled (e.g., via tokio::select with ctrl+c).
pub async fn run_js_event_loop(runtime: Arc<AsyncRuntime>) {
    debug!("Starting JavaScript event loop");

    // The idle() function processes pending JS jobs
    // For ctx.spawn() based timers, this is essential to process the spawned tasks
    loop {
        runtime.idle().await;

        // Yield periodically to ensure we can be interrupted by ctrl+c
        tokio::task::yield_now().await;
    }
}
