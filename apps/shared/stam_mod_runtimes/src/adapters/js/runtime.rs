use rquickjs::loader::{Loader, ModuleLoader, Resolver};
use rquickjs::{AsyncContext, AsyncRuntime, Ctx, Module, Object};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tracing::{debug, error, info};

use super::{JsRuntimeConfig, bindings};
use crate::api::AppApi;
use crate::{ModReturnValue, RuntimeAdapter};

/// Global registry mapping mod aliases (@mod-id) to their entry point paths
///
/// This is shared across all JavaScript runtime instances and allows mods
/// to import other mods using the `@mod-id` syntax, e.g.:
/// ```javascript
/// import { Manager } from "@js-helper";
/// ```
static MOD_ALIAS_REGISTRY: std::sync::LazyLock<RwLock<HashMap<String, PathBuf>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register a mod alias for cross-mod imports
///
/// # Arguments
/// * `mod_id` - The mod identifier (will be accessible as `@mod_id`)
/// * `entry_point` - The absolute path to the mod's entry point file
pub fn register_mod_alias(mod_id: &str, entry_point: PathBuf) {
    let alias = format!("@{}", mod_id);
    //debug!("Registering mod alias: {} -> {}", alias, entry_point.display());
    let mut registry = MOD_ALIAS_REGISTRY.write().unwrap();
    registry.insert(alias, entry_point);
}

/// Custom resolver that handles @mod-id imports
///
/// This resolver intercepts imports starting with `@` and resolves them
/// to the registered mod's entry point path. For all other imports,
/// it delegates to the standard file resolution.
#[derive(Clone)]
struct ModAliasResolver;

impl Resolver for ModAliasResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
    ) -> rquickjs::Result<String> {
        debug!("ModAliasResolver: resolve called with base='{}', name='{}'", base, name);

        // Check if this is a mod alias import (@mod-id)
        if name.starts_with('@') {
            let registry = MOD_ALIAS_REGISTRY.read().unwrap();

            // The alias might be just "@mod-id" or "@mod-id/subpath"
            let (alias, subpath) = if let Some(slash_pos) = name[1..].find('/') {
                let alias_end = slash_pos + 1;
                (&name[..alias_end], Some(&name[alias_end + 1..]))
            } else {
                (name, None)
            };

            if let Some(entry_point) = registry.get(alias) {
                let resolved = if let Some(sub) = subpath {
                    // @mod-id/subpath -> mod_dir/subpath
                    let mod_dir = entry_point.parent().unwrap_or(Path::new("."));
                    mod_dir.join(sub).to_string_lossy().to_string()
                } else {
                    // @mod-id -> mod's entry point
                    entry_point.to_string_lossy().to_string()
                };

                //debug!("ModAliasResolver: {} -> {}", name, resolved);
                return Ok(resolved);
            } else {
                error!("ModAliasResolver: Unknown mod alias '{}'. Available aliases: {:?}",
                    alias, registry.keys().collect::<Vec<_>>());
                return Err(rquickjs::Error::new_resolving(base, name));
            }
        }

        // For relative imports, resolve relative to base
        if name.starts_with('.') {
            let base_dir = Path::new(base).parent().unwrap_or(Path::new("."));
            let resolved = base_dir.join(name);
            let resolved_str = resolved.to_string_lossy().to_string();
            debug!("ModAliasResolver: relative '{}' (from '{}') -> '{}'", name, base, resolved_str);
            return Ok(resolved_str);
        }

        // For absolute or other imports, return as-is
        debug!("ModAliasResolver: passthrough '{}'", name);
        Ok(name.to_string())
    }
}

/// Custom filesystem loader for JavaScript modules
struct FilesystemLoader;

impl Loader for FilesystemLoader {
    fn load<'js>(&mut self, ctx: &rquickjs::Ctx<'js>, path: &str) -> rquickjs::Result<Module<'js>> {
        debug!("FilesystemLoader: Loading module from path: '{}'", path);

        // Try to read the file, with automatic .js extension fallback
        let (actual_path, content) = Self::read_with_js_fallback(path)?;

        debug!(
            "FilesystemLoader: Successfully read {} bytes from '{}'",
            content.len(),
            actual_path
        );

        // Compile and return the module using the actual path found
        Module::declare(ctx.clone(), actual_path.clone(), content).map_err(|e| {
            error!(
                "FilesystemLoader: Failed to declare module '{}': {:?}",
                actual_path, e
            );
            e
        })
    }
}

impl FilesystemLoader {
    /// Try to read a file, automatically adding .js extension if needed
    fn read_with_js_fallback(path: &str) -> rquickjs::Result<(String, String)> {
        // First, try the exact path
        if let Ok(content) = fs::read_to_string(path) {
            return Ok((path.to_string(), content));
        }

        // If the path doesn't have a .js extension, try adding it
        if !path.ends_with(".js") {
            let path_with_js = format!("{}.js", path);
            if let Ok(content) = fs::read_to_string(&path_with_js) {
                //debug!("FilesystemLoader: Resolved '{}' to '{}'", path, path_with_js);
                return Ok((path_with_js, content));
            }

            // Also try index.js in case it's a directory import
            let index_path = format!("{}/index.js", path);
            if let Ok(content) = fs::read_to_string(&index_path) {
                //debug!("FilesystemLoader: Resolved '{}' to '{}'", path, index_path);
                return Ok((index_path, content));
            }
        }

        // Nothing worked, return error with original path
        error!("FilesystemLoader: Failed to read file '{}' (also tried .js extension)", path);
        Err(rquickjs::Error::new_loading(path))
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
    async fn setup_global_apis(
        &self,
        context: &AsyncContext,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let game_data_dir = self.config.game_data_dir().clone();
        let game_config_dir = self.config.game_config_dir().clone();

        debug!("setup_global_apis: game_data_dir={:?}, game_config_dir={:?}",
            game_data_dir, game_config_dir);

        context
            .with(|ctx| {
                // Register console API
                debug!("Setting up console API...");
                bindings::setup_console_api(ctx.clone())?;
                debug!("Console API setup complete");

                // Register process API with game-specific directories
                debug!("Setting up process API...");
                let app_api = AppApi::new(game_data_dir, game_config_dir);
                bindings::setup_process_api(ctx.clone(), app_api)?;
                debug!("Process API setup complete");

                // Register timer API (setTimeout, setInterval, etc.)
                debug!("Setting up timer API...");
                bindings::setup_timer_api(ctx.clone())?;
                debug!("Timer API setup complete");

                Ok::<(), rquickjs::Error>(())
            })
            .await?;

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
    pub async fn load_mod_async(
        &mut self,
        mod_path: &Path,
        mod_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // debug!(
        //     "Loading JavaScript module: {} from {}",
        //     mod_id,
        //     mod_path.display()
        // );

        // Get the mod directory (parent of the entry point file)
        let mod_dir = mod_path
            .parent()
            .ok_or_else(|| {
                format!(
                    "Cannot determine mod directory for '{}'",
                    mod_path.display()
                )
            })?
            .to_path_buf();

        // Register mod alias for cross-mod imports (@mod-id syntax)
        // Use absolute path for reliable resolution - canonicalize to remove ./ and normalize
        let absolute_entry_point = fs::canonicalize(mod_path)
            .map_err(|e| format!("Failed to canonicalize path '{}': {}", mod_path.display(), e))?;
        register_mod_alias(mod_id, absolute_entry_point.clone());

        // Add mod directory to the list of search paths and update loader
        debug!("mod_dirs before: {:?}, checking if contains {:?}", self.mod_dirs, mod_dir);
        if !self.mod_dirs.contains(&mod_dir) {
            self.mod_dirs.push(mod_dir.clone());

            // Use ModAliasResolver for @mod-id imports, combined with FileResolver for relative imports
            let resolver = ModAliasResolver;

            let loader = (FilesystemLoader, ModuleLoader::default());
            debug!("Setting loader for runtime...");
            self.runtime.set_loader(resolver, loader).await;
            debug!("Loader set successfully");
        }

        // Create a new isolated AsyncContext for this mod
        let context = AsyncContext::full(&self.runtime).await?;

        // Setup global APIs for this mod's context
        self.setup_global_apis(&context).await?;

        // Set global __MOD_ID__ variable for console logging
        debug!("Setting __MOD_ID__ to '{}'", mod_id);
        context
            .with(|ctx| {
                ctx.globals().set("__MOD_ID__", mod_id)?;
                Ok::<(), rquickjs::Error>(())
            })
            .await?;
        debug!("__MOD_ID__ set successfully");

        // Use absolute path for the initial module import
        // This ensures the loader can find the file regardless of working directory
        let module_path_str = absolute_entry_point.to_string_lossy().to_string();

        let mod_id_owned = mod_id.to_string();

        debug!("Importing module from path: '{}'", module_path_str);

        // Read the entry point file content
        let entry_content = fs::read_to_string(&absolute_entry_point)
            .map_err(|e| format!("Failed to read entry point '{}': {}", module_path_str, e))?;

        debug!("Read {} bytes from entry point", entry_content.len());

        // Load the module from the filesystem
        // Use Result<String, String> for ParallelSend compatibility
        let result: Result<String, String> = context
            .with(|ctx| {
                debug!("Inside context, declaring module for '{}'", mod_id_owned);

                // Declare the module with the file content
                match Module::declare(ctx.clone(), module_path_str.clone(), entry_content) {
                    Ok(module) => {
                        debug!("Module declared, now evaluating...");
                        // Evaluate the module to execute it
                        match module.eval() {
                            Ok((evaluated_module, promise)) => match promise.finish::<()>() {
                                Ok(_) => {
                                    debug!("Module '{}' evaluated successfully", mod_id_owned);
                                    // Store the module namespace in a global variable for later access
                                    // This avoids re-importing the module
                                    let namespace_key = format!("__MODULE_NS_{}__", mod_id_owned.replace("-", "_"));
                                    if let Ok(namespace) = evaluated_module.namespace() {
                                        if let Err(e) = ctx.globals().set(&namespace_key, namespace) {
                                            error!("Failed to store module namespace: {:?}", e);
                                        }
                                    }
                                    Ok(module_path_str.clone())
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("\n{}", error_msg);
                                    Err(format!("JavaScript error in mod '{}'", mod_id_owned))
                                }
                            },
                            Err(e) => {
                                let error_msg = Self::format_js_error(&ctx, &e);
                                error!("\n{}", error_msg);
                                Err(format!("JavaScript error evaluating mod '{}'", mod_id_owned))
                            }
                        }
                    }
                    Err(e) => {
                        let error_msg = Self::format_js_error(&ctx, &e);
                        error!("\n{}", error_msg);
                        Err(format!("JavaScript error declaring mod '{}'", mod_id_owned))
                    }
                }
            })
            .await;

        let stored_module_path = result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

        // Store the loaded mod
        self.loaded_mods.insert(
            mod_id.to_string(),
            LoadedMod {
                context,
                module_path: stored_module_path,
                mod_dir,
            },
        );

        Ok(())
    }

    /// Call a mod function asynchronously
    pub async fn call_mod_function_async(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        //debug!("Calling JavaScript function '{}' for mod '{}'", function_name, mod_id);

        let loaded_mod = self
            .loaded_mods
            .get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        let mod_id_owned = mod_id.to_string();
        let function_name_owned = function_name.to_string();

        // Use Result<(), String> for ParallelSend compatibility
        // Get the stored module namespace from globals instead of re-importing
        let namespace_key = format!("__MODULE_NS_{}__", mod_id.replace("-", "_"));

        let result: Result<(), String> = loaded_mod
            .context
            .with(|ctx| {
                // Get the stored module namespace from globals
                match ctx.globals().get::<_, Object>(&namespace_key) {
                    Ok(module_namespace) => {
                        match module_namespace
                            .get::<_, rquickjs::Function>(&function_name_owned)
                        {
                            Ok(func) => {
                                match func.call::<(), ()>(()) {
                                    Ok(_) => {
                                        //debug!("Function '{}' executed successfully for mod '{}'", function_name_owned, mod_id_owned);
                                        Ok(())
                                    }
                                    Err(e) => {
                                        let error_msg = Self::format_js_error(&ctx, &e);
                                        error!("\n{}", error_msg);
                                        Err(format!(
                                            "JavaScript error in '{}' for mod '{}'",
                                            function_name_owned, mod_id_owned
                                        ))
                                    }
                                }
                            }
                            Err(_) => {
                                debug!(
                                    "Function '{}' not found or not exported for mod '{}'",
                                    function_name_owned, mod_id_owned
                                );
                                Ok(())
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to get module namespace '{}': {:?}", namespace_key, e);
                        Err(format!(
                            "Failed to get module namespace for mod '{}'",
                            mod_id_owned
                        ))
                    }
                }
            })
            .await;

        result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    }

    /// Call a mod function asynchronously with return value
    pub async fn call_mod_function_with_return_async(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        //debug!("Calling JavaScript function '{}' for mod '{}' with return", function_name, mod_id);

        let loaded_mod = self
            .loaded_mods
            .get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        let mod_id_owned = mod_id.to_string();
        let function_name_owned = function_name.to_string();

        // Get the stored module namespace from globals instead of re-importing
        let namespace_key = format!("__MODULE_NS_{}__", mod_id.replace("-", "_"));

        // Use Result<ModReturnValue, String> for ParallelSend compatibility
        let result: Result<ModReturnValue, String> = loaded_mod
            .context
            .with(|ctx| {
                // Get the stored module namespace from globals
                match ctx.globals().get::<_, Object>(&namespace_key) {
                    Ok(module_namespace) => {
                        match module_namespace.get::<_, rquickjs::Function>(&function_name_owned) {
                            Ok(func) => match func.call::<(), String>(()) {
                                Ok(value) => {
                                    debug!(
                                        "Function '{}' returned string for mod '{}'",
                                        function_name_owned, mod_id_owned
                                    );
                                    Ok(ModReturnValue::String(value))
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("\n{}", error_msg);
                                    Err(format!(
                                        "JavaScript error in '{}' for mod '{}'",
                                        function_name_owned, mod_id_owned
                                    ))
                                }
                            },
                            Err(_) => {
                                debug!(
                                    "Function '{}' not found or not exported for mod '{}'",
                                    function_name_owned, mod_id_owned
                                );
                                Ok(ModReturnValue::None)
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to get module namespace '{}': {:?}", namespace_key, e);
                        Err(format!(
                            "Failed to get module namespace for mod '{}'",
                            mod_id_owned
                        ))
                    }
                }
            })
            .await;

        result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    }
}

// Synchronous trait implementation that wraps async calls
impl RuntimeAdapter for JsRuntimeAdapter {
    fn load_mod(
        &mut self,
        mod_path: &Path,
        mod_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.load_mod_async(mod_path, mod_id))
        })
    }

    fn call_mod_function(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.call_mod_function_async(mod_id, function_name))
        })
    }

    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.call_mod_function_with_return_async(mod_id, function_name))
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
