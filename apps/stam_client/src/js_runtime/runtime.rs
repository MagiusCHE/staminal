use rquickjs::{Context, Runtime};
use std::path::Path;
use std::fs;
use tracing::{info, error, debug, warn};

use super::{console_api, process_api, ScriptRuntimeConfig};

/// JavaScript runtime manager for mod execution using QuickJS
pub struct JsRuntime {
    runtime: Runtime,
    context: Context,
    config: ScriptRuntimeConfig,
}

impl JsRuntime {
    /// Create a new JavaScript runtime instance with QuickJS
    ///
    /// # Arguments
    /// * `config` - Runtime configuration containing app paths and game ID
    pub fn new(config: ScriptRuntimeConfig) -> Result<Self, Box<dyn std::error::Error>> {
        debug!("Initializing QuickJS runtime for mods");

        let runtime = Runtime::new()?;
        let context = Context::full(&runtime)?;

        let mut js_runtime = Self {
            runtime,
            context,
            config,
        };

        // Setup global APIs
        js_runtime.setup_global_apis()?;

        info!("JavaScript runtime initialized successfully");
        Ok(js_runtime)
    }

    /// Setup all global APIs available to mods
    fn setup_global_apis(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let game_data_dir = self.config.game_data_dir().clone();
        let game_config_dir = self.config.game_config_dir().clone();

        self.context.with(|ctx| {
            // Register console API
            console_api::setup_console_api(ctx.clone())?;

            // Register process API with game-specific directories
            process_api::setup_process_api(ctx.clone(), game_data_dir, game_config_dir)?;

            // Future APIs will be registered here:
            // client_api::setup_client_api(ctx.clone())?;
            // events_api::setup_events_api(ctx.clone())?;
            // etc.

            Ok::<(), rquickjs::Error>(())
        })?;

        Ok(())
    }

    /// Load and execute a JavaScript module file
    ///
    /// # Arguments
    /// * `mod_path` - Path to the JavaScript file (e.g., "mods/my-mod/main.js")
    /// * `mod_id` - Identifier for the mod (used in logging)
    pub fn load_module(
        &mut self,
        mod_path: &Path,
        mod_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Loading JavaScript module: {} from {}", mod_id, mod_path.display());

        // Read JavaScript file
        let js_code = fs::read_to_string(mod_path)
            .map_err(|e| format!("Failed to read mod file '{}': {}", mod_path.display(), e))?;

        // Set global __MOD_ID__ variable for console logging
        self.context.with(|ctx| {
            ctx.globals().set("__MOD_ID__", mod_id)?;
            Ok::<(), rquickjs::Error>(())
        })?;

        // Execute JavaScript code
        self.context.with(|ctx| {
            match ctx.eval::<(), _>(js_code.as_str()) {
                Ok(_) => {
                    info!("Mod '{}' loaded successfully", mod_id);
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to load mod '{}': {}", mod_id, e);
                    Err(format!("JavaScript error in mod '{}': {}", mod_id, e).into())
                }
            }
        })
    }

    /// Call a JavaScript function by name
    ///
    /// # Arguments
    /// * `function_name` - Name of the global function to call (e.g., "onAttach")
    pub fn call_function(&mut self, function_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Calling JavaScript function: {}", function_name);

        self.context.with(|ctx| {
            let globals = ctx.globals();

            // Check if function exists
            let func: Option<rquickjs::Function> = globals.get(function_name).ok();

            match func {
                Some(func) => {
                    // Call function with no arguments
                    match func.call::<(), ()>(()) {
                        Ok(_) => {
                            debug!("Function '{}' executed successfully", function_name);
                            Ok(())
                        }
                        Err(e) => {
                            error!("Error calling function '{}': {}", function_name, e);
                            Err(format!("JavaScript error in '{}': {}", function_name, e).into())
                        }
                    }
                }
                None => {
                    warn!("Function '{}' not found in JavaScript context", function_name);
                    // Not an error - function might be optional
                    Ok(())
                }
            }
        })
    }

    /// Call a JavaScript function for a specific mod
    ///
    /// This sets __MOD_ID__ before calling the function, ensuring proper logging context
    ///
    /// # Arguments
    /// * `function_name` - Name of the global function to call
    /// * `mod_id` - ID of the mod (used for logging and __MOD_ID__ context)
    pub fn call_function_for_mod(
        &mut self,
        function_name: &str,
        mod_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Calling JavaScript function '{}' for mod '{}'", function_name, mod_id);

        // Set __MOD_ID__ for this mod
        self.context.with(|ctx| {
            ctx.globals().set("__MOD_ID__", mod_id)?;
            Ok::<(), rquickjs::Error>(())
        })?;

        // Call the function
        self.context.with(|ctx| {
            let globals = ctx.globals();
            let func: Option<rquickjs::Function> = globals.get(function_name).ok();

            match func {
                Some(func) => {
                    match func.call::<(), ()>(()) {
                        Ok(_) => {
                            debug!("Function '{}' executed successfully for mod '{}'", function_name, mod_id);
                            Ok(())
                        }
                        Err(e) => {
                            error!("Error calling function '{}' for mod '{}': {}", function_name, mod_id, e);
                            Err(format!("JavaScript error in '{}' for mod '{}': {}", function_name, mod_id, e).into())
                        }
                    }
                }
                None => {
                    debug!("Function '{}' not found for mod '{}' (optional)", function_name, mod_id);
                    // Not an error - function might be optional
                    Ok(())
                }
            }
        })
    }

    /// Call a JavaScript function with a single string argument
    #[allow(dead_code)]
    pub fn call_function_with_arg(
        &mut self,
        function_name: &str,
        arg: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Calling JavaScript function: {}(\"{}\")", function_name, arg);

        self.context.with(|ctx| {
            let globals = ctx.globals();
            let func: Option<rquickjs::Function> = globals.get(function_name).ok();

            match func {
                Some(func) => {
                    match func.call::<_, ()>((arg,)) {
                        Ok(_) => {
                            debug!("Function '{}' executed successfully", function_name);
                            Ok(())
                        }
                        Err(e) => {
                            error!("Error calling function '{}': {}", function_name, e);
                            Err(format!("JavaScript error in '{}': {}", function_name, e).into())
                        }
                    }
                }
                None => {
                    warn!("Function '{}' not found in JavaScript context", function_name);
                    Ok(())
                }
            }
        })
    }

    /// Evaluate JavaScript code directly (useful for testing)
    #[allow(dead_code)]
    pub fn eval(&mut self, code: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.context.with(|ctx| {
            match ctx.eval::<(), _>(code) {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("JavaScript error: {}", e).into())
            }
        })
    }
}
