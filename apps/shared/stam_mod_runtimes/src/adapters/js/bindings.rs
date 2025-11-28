//! JavaScript bindings for runtime APIs
//!
//! This module provides the bridge between Rust APIs and JavaScript contexts.
//!
//! # Timer System Architecture
//!
//! The timer system (setTimeout, setInterval, etc.) uses a global registry approach:
//!
//! - **`NEXT_TIMER_ID`**: Atomic counter ensuring unique IDs across ALL runtime instances
//!   (JavaScript, Lua, C#, etc.). This prevents ID collisions even in multi-runtime scenarios.
//!
//! - **`TIMER_ABORT_HANDLES`**: Global registry mapping timer IDs to cancellation handles.
//!   Since IDs are globally unique, this allows `clearTimeout(id)` to work correctly
//!   regardless of which runtime or mod created the timer.
//!
//! This design supports:
//! - Multiple runtime types (JS + Lua + C#) running simultaneously
//! - Multiple instances of the same runtime type (e.g., for testing)
//! - Thread-safe timer creation and cancellation
//!
//! Note: When a QuickJS runtime is dropped, spawned tasks are automatically cancelled,
//! but their entries remain in TIMER_ABORT_HANDLES until the task cleanup runs.
//! This is acceptable because the Notify handles are small and will be cleaned up
//! when the spawned task completes or is aborted.
//!
//! # JavaScript Glue Code
//!
//! The JavaScript glue code (console formatters, error handlers, etc.) is loaded from
//! external .js files in the `glue/` directory. These files are concatenated at compile
//! time by build.rs and embedded into the binary.

use crate::api::{AppApi, ConsoleApi, LocaleApi, NetworkApi, RequestUriProtocol, SystemApi, SystemEvents, ModSide};

/// JavaScript glue code - embedded at compile time from src/adapters/js/glue/*.js
/// This code sets up console, error handlers, and other runtime utilities.
const JS_GLUE_CODE: &str = include_str!("glue/main.js");
use rquickjs::{Array, Ctx, Function, JsLifetime, Object, Value, class::Trace, function::{Opt, Rest}};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::RwLock;
use tokio::sync::Notify;

/// Unique timer ID counter - globally atomic to ensure unique IDs across ALL runtimes
/// (JavaScript, Lua, C#, etc.) and all instances of each runtime type.
static NEXT_TIMER_ID: AtomicU32 = AtomicU32::new(1);

/// Generate a new unique timer ID
///
/// This function is public so that other runtime adapters (Lua, C#, etc.)
/// can use the same global timer ID counter, ensuring no ID collisions
/// across different runtime types.
pub fn next_timer_id() -> u32 {
    NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst)
}

/// Global timer cancellation registry
/// Maps timer_id -> Notify handle for cancellation
///
/// This is global (not per-runtime) because timer IDs are globally unique.
/// This allows clearTimeout/clearInterval to work correctly even if called
/// from a different context than the one that created the timer.
static TIMER_ABORT_HANDLES: std::sync::LazyLock<std::sync::Mutex<HashMap<u32, Arc<Notify>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// Unique temp file ID counter for generating unique file names
static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

/// Temp file manager for tracking and cleaning up temporary files created during downloads
///
/// This manager tracks all temp files created by `network.download()` calls.
/// Files are stored in `home_dir/tmp/` with unique names to avoid collisions.
/// All tracked files are cleaned up when `cleanup()` is called.
#[derive(Clone, Default)]
pub struct TempFileManager {
    /// List of temp file paths that need to be cleaned up
    files: Arc<RwLock<Vec<PathBuf>>>,
    /// Base temp directory path (home_dir/tmp)
    temp_dir: Arc<RwLock<Option<PathBuf>>>,
}

impl TempFileManager {
    /// Create a new TempFileManager
    pub fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(Vec::new())),
            temp_dir: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the temp directory path (should be called with home_dir/tmp)
    pub fn set_temp_dir(&self, path: PathBuf) {
        let mut temp_dir = self.temp_dir.write().unwrap();
        *temp_dir = Some(path);
    }

    /// Get the temp directory path
    pub fn get_temp_dir(&self) -> Option<PathBuf> {
        let temp_dir = self.temp_dir.read().unwrap();
        temp_dir.clone()
    }

    /// Create a unique temp file and write data to it
    ///
    /// Returns the path to the created file, or an error message
    pub fn create_temp_file(&self, data: &[u8], original_file_name: Option<&str>) -> Result<PathBuf, String> {
        let temp_dir = self.get_temp_dir()
            .ok_or_else(|| "Temp directory not configured".to_string())?;

        // Ensure temp directory exists
        if !temp_dir.exists() {
            std::fs::create_dir_all(&temp_dir)
                .map_err(|e| format!("Failed to create temp directory: {}", e))?;
        }

        // Generate unique file name
        let unique_id = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::SeqCst);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        let file_name = if let Some(name) = original_file_name {
            // Preserve original extension if possible
            let ext = std::path::Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("tmp");
            format!("download_{}_{}.{}", timestamp, unique_id, ext)
        } else {
            format!("download_{}_{}.tmp", timestamp, unique_id)
        };

        let file_path = temp_dir.join(&file_name);

        // Write data to file
        std::fs::write(&file_path, data)
            .map_err(|e| format!("Failed to write temp file: {}", e))?;

        // Track the file for cleanup
        let mut files = self.files.write().unwrap();
        files.push(file_path.clone());

        Ok(file_path)
    }

    /// Clean up all tracked temp files
    ///
    /// This removes all temp files that were created by this manager.
    /// Errors are logged but don't stop the cleanup process.
    pub fn cleanup(&self) {
        let mut files = self.files.write().unwrap();

        for file_path in files.drain(..) {
            if file_path.exists() {
                if let Err(e) = std::fs::remove_file(&file_path) {
                    tracing::warn!("Failed to cleanup temp file {:?}: {}", file_path, e);
                } else {
                    tracing::debug!("Cleaned up temp file: {:?}", file_path);
                }
            }
        }
    }

    /// Get the number of tracked temp files
    pub fn file_count(&self) -> usize {
        let files = self.files.read().unwrap();
        files.len()
    }
}

impl Drop for TempFileManager {
    fn drop(&mut self) {
        // Automatically cleanup when the manager is dropped
        self.cleanup();
    }
}

/// Name of the global JavaScript object used to store event handlers
const JS_EVENT_HANDLERS_MAP: &str = "__eventHandlers";

/// Store a JavaScript function handler in the context's handler map
pub fn store_js_handler<'js>(
    ctx: &Ctx<'js>,
    handler_id: u64,
    handler: Function<'js>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let handlers_map: Object = globals.get(JS_EVENT_HANDLERS_MAP)?;
    handlers_map.set(handler_id.to_string(), handler)?;
    Ok(())
}

/// Remove a JavaScript function handler from the context's handler map
pub fn remove_js_handler(ctx: &Ctx<'_>, handler_id: u64) -> rquickjs::Result<bool> {
    let globals = ctx.globals();
    let handlers_map: Object = globals.get(JS_EVENT_HANDLERS_MAP)?;
    let key = handler_id.to_string();
    let exists: bool = handlers_map.contains_key(&key)?;
    if exists {
        // Delete the property by setting to undefined
        handlers_map.set(&key, rquickjs::Undefined)?;
    }
    Ok(exists)
}

/// Get a JavaScript function handler by ID from the context's handler map
pub fn get_js_handler<'js>(
    ctx: &Ctx<'js>,
    handler_id: u64,
) -> rquickjs::Result<Option<Function<'js>>> {
    let globals = ctx.globals();
    let handlers_map: Object = globals.get(JS_EVENT_HANDLERS_MAP)?;
    let key = handler_id.to_string();
    let handler: rquickjs::Value = handlers_map.get(&key)?;
    if handler.is_undefined() || handler.is_null() {
        Ok(None)
    } else {
        Ok(handler.into_function())
    }
}

/// Initialize the event handlers map in a JavaScript context
pub fn init_event_handlers_map(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let handlers_map = Object::new(ctx.clone())?;
    ctx.globals().set(JS_EVENT_HANDLERS_MAP, handlers_map)?;
    Ok(())
}

/// Setup console API in the JavaScript context
///
/// Provides console.log, console.error, console.warn, console.info, console.debug
/// All functions accept variadic arguments and read the global __GAME_ID__ (optional) and __MOD_ID__ variables
pub fn setup_console_api(ctx: Ctx) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    // Create native console object with raw string-based functions
    let console_native = Object::new(ctx.clone())?;

    // Native _log function - accepts a single pre-formatted string message
    let log_fn = Function::new(ctx.clone(), |ctx: Ctx, message: String| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());
        ConsoleApi::log(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console_native.set("_log", log_fn)?;

    // Native _error function
    let error_fn = Function::new(ctx.clone(), |ctx: Ctx, message: String| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());
        ConsoleApi::error(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console_native.set("_error", error_fn)?;

    // Native _warn function
    let warn_fn = Function::new(ctx.clone(), |ctx: Ctx, message: String| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());
        ConsoleApi::warn(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console_native.set("_warn", warn_fn)?;

    // Native _info function
    let info_fn = Function::new(ctx.clone(), |ctx: Ctx, message: String| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());
        ConsoleApi::info(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console_native.set("_info", info_fn)?;

    // Native _debug function
    let debug_fn = Function::new(ctx.clone(), |ctx: Ctx, message: String| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());
        ConsoleApi::debug(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console_native.set("_debug", debug_fn)?;

    // Register native console object
    globals.set("__console_native", console_native)?;

    // Execute the JavaScript glue code (loaded from external .js files at compile time)
    // This sets up console wrappers, error handlers, and other runtime utilities
    ctx.eval::<(), _>(JS_GLUE_CODE)?;

    Ok(())
}

/// Setup process API in the JavaScript context
///
/// Provides process.app.data_path and process.app.config_path
pub fn setup_process_api(ctx: Ctx, app_api: AppApi) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    // Create process object
    let process = Object::new(ctx.clone())?;

    // Create process.app object
    let app = Object::new(ctx.clone())?;

    // Get paths from AppApi
    let data_path = app_api.data_path();
    let config_path = app_api.config_path();

    // Set process.app.data_path
    app.set("data_path", data_path)?;

    // Set process.app.config_path
    app.set("config_path", config_path)?;

    // Register app object in process
    process.set("app", app)?;

    // Register process object globally
    globals.set("process", process)?;

    Ok(())
}

/// Internal implementation of setTimeout/setInterval
/// This is a separate function to properly handle lifetimes
fn set_timeout_interval<'js>(
    ctx: Ctx<'js>,
    cb: Function<'js>,
    msec: Option<u64>,
    is_interval: bool,
) -> rquickjs::Result<u32> {
    let id = next_timer_id();
    //let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());

    // Enforce minimum 4ms delay as per HTML5 spec
    let delay = msec.unwrap_or(0).max(4);

    let abort = Arc::new(Notify::new());
    let abort_ref = abort.clone();

    // Store abort handle for clearTimeout/clearInterval
    {
        let mut handles = TIMER_ABORT_HANDLES.lock().unwrap();
        handles.insert(id, abort);
    }

    //let timer_type = if is_interval { "setInterval" } else { "setTimeout" };
    //tracing::debug!("{}: timer {} scheduled with {}ms delay for mod '{}'", timer_type, id, delay, mod_id);

    // Spawn async task in the JS context
    ctx.spawn(async move {
        let duration = tokio::time::Duration::from_millis(delay);

        loop {
            tokio::select! {
                biased;

                // Check for cancellation
                _ = abort_ref.notified() => {
                    //tracing::debug!("Timer {} aborted", id);
                    break;
                }

                // Wait for timer
                _ = tokio::time::sleep(duration) => {
                    //tracing::trace!("Timer {} fired after {}ms", id, delay);

                    // Execute the callback
                    if let Err(_err) = cb.call::<(), ()>(()) {
                        //tracing::error!("Timer {} callback error: {:?}", id, _err);
                        break;
                    }

                    // For setTimeout, run only once
                    if !is_interval {
                        break;
                    }
                }
            }
        }

        // Cleanup abort handle
        {
            let mut handles = TIMER_ABORT_HANDLES.lock().unwrap();
            handles.remove(&id);
        }

        // Cleanup
        drop(cb);
        drop(abort_ref);
        //tracing::trace!("Timer {} completed", id);
    });

    Ok(id)
}

/// Clear a timer by ID
///
/// This function is public so that other runtime adapters (Lua, C#, etc.)
/// can cancel timers using the same global registry.
pub fn clear_timer(timer_id: u32) {
    let abort = {
        let handles = TIMER_ABORT_HANDLES.lock().unwrap();
        handles.get(&timer_id).cloned()
    };

    if let Some(abort) = abort {
        abort.notify_one();
    }
}

/// Register an abort handle for a timer
///
/// This function is public so that other runtime adapters (Lua, C#, etc.)
/// can register their timer abort handles in the global registry.
pub fn register_timer_abort_handle(timer_id: u32, abort: Arc<Notify>) {
    let mut handles = TIMER_ABORT_HANDLES.lock().unwrap();
    handles.insert(timer_id, abort);
}

/// Remove an abort handle for a timer (called when timer completes)
///
/// This function is public so that other runtime adapters (Lua, C#, etc.)
/// can clean up their timer abort handles from the global registry.
pub fn remove_timer_abort_handle(timer_id: u32) {
    let mut handles = TIMER_ABORT_HANDLES.lock().unwrap();
    handles.remove(&timer_id);
}

/// Setup timer API in the JavaScript context
///
/// Provides setTimeout, setInterval, clearTimeout, clearInterval functions.
/// Uses ctx.spawn() for proper async execution within the QuickJS runtime.
/// Returns numeric timer IDs like browser APIs.
pub fn setup_timer_api(ctx: Ctx) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    // setTimeout(callback, delay?) -> number
    let set_timeout_fn = Function::new(ctx.clone(), set_timeout_interval_wrapper::<false>)?;
    globals.set("setTimeout", set_timeout_fn)?;

    // setInterval(callback, interval?) -> number
    let set_interval_fn = Function::new(ctx.clone(), set_timeout_interval_wrapper::<true>)?;
    globals.set("setInterval", set_interval_fn)?;

    // clearTimeout(timerId) - cancels a pending timeout
    let clear_timeout_fn = Function::new(ctx.clone(), |_ctx: Ctx, timer_id: u32| {
        //tracing::debug!("clearTimeout: cancelling timer {}", timer_id);
        clear_timer(timer_id);
    })?;
    globals.set("clearTimeout", clear_timeout_fn)?;

    // clearInterval(intervalId) - cancels a pending interval
    let clear_interval_fn = Function::new(ctx.clone(), |_ctx: Ctx, timer_id: u32| {
        //tracing::debug!("clearInterval: cancelling interval {}", timer_id);
        clear_timer(timer_id);
    })?;
    globals.set("clearInterval", clear_interval_fn)?;

    Ok(())
}

/// Wrapper function for setTimeout/setInterval that can be used with Function::new
fn set_timeout_interval_wrapper<'js, const IS_INTERVAL: bool>(
    ctx: Ctx<'js>,
    cb: Function<'js>,
    msec: Option<u64>,
) -> rquickjs::Result<u32> {
    set_timeout_interval(ctx, cb, msec, IS_INTERVAL)
}

/// JavaScript System API class
///
/// This class is exposed to JavaScript as the `system` global object.
/// It provides methods to query information about loaded mods.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct SystemJS {
    #[qjs(skip_trace)]
    system_api: SystemApi,
}

#[rquickjs::methods]
impl SystemJS {
    /// Get information about all loaded mods
    ///
    /// Returns an array of objects with properties:
    /// - id: string
    /// - version: string
    /// - name: string
    /// - description: string
    /// - mod_type: string | null
    /// - priority: number
    /// - bootstrapped: boolean
    #[qjs(rename = "get_mods")]
    pub fn get_mods<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Array<'js>> {
        //tracing::debug!("SystemJS::get_mods called");

        let mods = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.system_api.get_mods()
        })) {
            Ok(mods) => mods,
            Err(e) => {
                //tracing::error!("Panic in get_mods: {:?}", e);
                return Err(rquickjs::Error::Exception);
            }
        };

        //tracing::debug!("SystemJS::get_mods got {} mods", mods.len());

        let array = Array::new(ctx.clone())?;

        for (idx, mod_info) in mods.iter().enumerate() {
            let obj = Object::new(ctx.clone())?;
            obj.set("id", mod_info.id.as_str())?;
            obj.set("version", mod_info.version.as_str())?;
            obj.set("name", mod_info.name.as_str())?;
            obj.set("description", mod_info.description.as_str())?;
            obj.set("mod_type", mod_info.mod_type.as_deref())?;
            obj.set("priority", mod_info.priority)?;
            obj.set("bootstrapped", mod_info.bootstrapped)?;
            obj.set("loaded", mod_info.loaded)?;
            obj.set("exists", mod_info.exists)?;
            obj.set("download_url", mod_info.download_url.as_deref())?;
            array.set(idx, obj)?;
        }

        tracing::debug!("SystemJS::get_mods returning array");
        Ok(array)
    }

    /// Register an event handler for a system event (number) or custom event (string)
    ///
    /// # Arguments
    /// * `event` - Either a SystemEvents enum value (number) or a custom event name (string)
    /// * `handler` - The callback function to invoke
    /// * `priority` - Handler priority (lower numbers execute first)
    /// * `protocol` - (Optional) Protocol filter string for RequestUri ("stam://", "http://", or "" for all)
    /// * `route` - (Optional) Route prefix filter for RequestUri
    ///
    /// # Returns
    /// Unique handler ID for later removal
    #[qjs(rename = "register_event")]
    pub fn register_event<'js>(
        &self,
        ctx: Ctx<'js>,
        event: rquickjs::Value<'js>,
        handler: Function<'js>,
        priority: i32,
        protocol_str: Opt<String>,
        route: Opt<String>,
    ) -> rquickjs::Result<u64> {
        // Get the current mod_id from context globals
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());

        // Extract optional values
        let protocol_str = protocol_str.0;
        let route = route.0;

        // Determine if this is a system event (number) or custom event (string)
        if let Some(event_num) = event.as_int() {
            // System event (number)
            let event_u32 = event_num as u32;

            tracing::debug!(
                "SystemJS::register_event called: mod={}, event={}, priority={}, protocol={:?}, route={:?}",
                mod_id,
                event_u32,
                priority,
                protocol_str,
                route
            );

            // Validate event type
            let event_type = match SystemEvents::from_u32(event_u32) {
                Some(e) => e,
                None => {
                    tracing::error!("Invalid event type: {}", event_u32);
                    return Err(rquickjs::Error::Exception);
                }
            };

            // Parse protocol filter
            let protocol = match protocol_str.as_deref() {
                Some("stam://") | Some("stam") => RequestUriProtocol::Stam,
                Some("http://") | Some("https://") | Some("http") => RequestUriProtocol::Http,
                Some("") | None => RequestUriProtocol::All,
                Some(other) => {
                    tracing::warn!("Unknown protocol filter '{}', using All", other);
                    RequestUriProtocol::All
                }
            };

            // Register the handler with the event dispatcher
            let handler_id = self.system_api.event_dispatcher().register_handler(
                event_type,
                &mod_id,
                priority,
                protocol,
                route.unwrap_or_default(),
            );

            // Store the handler function in the context's handler map
            store_js_handler(&ctx, handler_id, handler)?;

            tracing::info!(
                "Registered event handler: mod={}, event={:?}, handler_id={}, priority={}",
                mod_id,
                event_type,
                handler_id,
                priority
            );

            Ok(handler_id)
        } else if let Some(event_name) = event.as_string() {
            // Custom event (string)
            let event_name_str = event_name.to_string()?;

            tracing::debug!(
                "SystemJS::register_event (custom) called: mod={}, event_name={}, priority={}",
                mod_id,
                event_name_str,
                priority
            );

            // Register the handler with the event dispatcher
            let handler_id = self.system_api.event_dispatcher().register_custom_handler(
                &event_name_str,
                &mod_id,
                priority,
            );

            // Store the handler function in the context's handler map
            store_js_handler(&ctx, handler_id, handler)?;

            tracing::info!(
                "Registered custom event handler: mod={}, event_name={}, handler_id={}, priority={}",
                mod_id,
                event_name_str,
                handler_id,
                priority
            );

            Ok(handler_id)
        } else {
            tracing::error!("register_event: first argument must be a number (SystemEvents) or string (custom event name)");
            Err(rquickjs::Error::Exception)
        }
    }

    /// Send/dispatch a custom event to all registered handlers (async)
    ///
    /// This function triggers all handlers registered for the given event name,
    /// passing the provided arguments to each handler.
    ///
    /// # Arguments
    /// * `event_name` - The custom event name to dispatch
    /// * `args` - Variadic arguments to pass to handlers (will be JSON-serialized)
    ///
    /// # Returns
    /// Promise that resolves when all handlers have completed
    #[qjs(rename = "send_event")]
    pub async fn send_event<'js>(&self, ctx: Ctx<'js>, event_name: String, args: Rest<Value<'js>>) -> rquickjs::Result<()> {
        // Convert each JS value to JSON string
        let json_args: Vec<String> = args.0.iter()
            .map(|v| ctx.json_stringify(v.clone())
                .ok()
                .flatten()
                .map(|s| s.to_string().unwrap_or_default())
                .unwrap_or_else(|| "null".to_string()))
            .collect();

        tracing::debug!("SystemJS::send_event called: event_name={}, args_count={}", event_name, json_args.len());

        let result = self.system_api.event_dispatcher().request_send_event(event_name.clone(), json_args).await;

        match result {
            Ok(()) => {
                tracing::debug!("Event '{}' dispatched successfully", event_name);
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to dispatch event '{}': {}", event_name, e);
                Err(rquickjs::Error::Exception)
            }
        }
    }

    /// Unregister an event handler
    ///
    /// # Arguments
    /// * `handler_id` - The handler ID returned from register_event
    ///
    /// # Returns
    /// true if the handler was found and removed, false otherwise
    #[qjs(rename = "unregister_event")]
    pub fn unregister_event(&self, ctx: Ctx<'_>, handler_id: u64) -> rquickjs::Result<bool> {
        tracing::debug!(
            "SystemJS::unregister_event called: handler_id={}",
            handler_id
        );

        // Remove from event dispatcher
        let removed = self
            .system_api
            .event_dispatcher()
            .unregister_handler(handler_id);

        // Remove the handler function from the context's map
        if removed {
            remove_js_handler(&ctx, handler_id)?;
            tracing::info!("Unregistered event handler: handler_id={}", handler_id);
        }

        Ok(removed)
    }

    /// Exit the application immediately with the specified exit code
    ///
    /// # Arguments
    /// * `code` - The exit code (0 = success, non-zero = error)
    ///
    /// # Note
    /// This function does not return - it terminates the process immediately
    #[qjs(rename = "exit")]
    pub fn exit(&self, code: i32) {
        tracing::info!("SystemJS::exit called with code {}", code);
        std::process::exit(code);
    }

    /// Get mod packages filtered by side (client or server)
    ///
    /// # Arguments
    /// * `side` - ModSides enum value (0 = Client, 1 = Server)
    ///
    /// # Returns
    /// Array of mod package info objects with properties:
    /// - id: string
    /// - sha512: string
    /// - path: string
    /// - manifest: object with name, version, description, entry_point, etc.
    #[qjs(rename = "get_mod_packages")]
    pub fn get_mod_packages<'js>(&self, ctx: Ctx<'js>, side: u32) -> rquickjs::Result<Array<'js>> {
        let mod_side = match ModSide::from_u32(side) {
            Some(s) => s,
            None => {
                tracing::error!("Invalid ModSide value: {}", side);
                return Err(rquickjs::Error::Exception);
            }
        };

        let packages = self.system_api.get_mod_packages(mod_side);
        tracing::debug!("SystemJS::get_mod_packages called: side={:?}, found {} packages", mod_side, packages.len());

        let array = Array::new(ctx.clone())?;

        for (idx, pkg) in packages.iter().enumerate() {
            let obj = Object::new(ctx.clone())?;
            obj.set("id", pkg.id.as_str())?;
            obj.set("sha512", pkg.sha512.as_str())?;
            obj.set("path", pkg.path.as_str())?;

            // Create manifest object
            let manifest_obj = Object::new(ctx.clone())?;
            manifest_obj.set("name", pkg.manifest.name.as_str())?;
            manifest_obj.set("version", pkg.manifest.version.as_str())?;
            manifest_obj.set("description", pkg.manifest.description.as_str())?;
            manifest_obj.set("entry_point", pkg.manifest.entry_point.as_str())?;
            manifest_obj.set("priority", pkg.manifest.priority)?;
            if let Some(ref mod_type) = pkg.manifest.mod_type {
                manifest_obj.set("type", mod_type.as_str())?;
            }

            obj.set("manifest", manifest_obj)?;
            array.set(idx, obj)?;
        }

        Ok(array)
    }

    /// Get the file path for a mod package by ID
    ///
    /// # Arguments
    /// * `mod_id` - The mod identifier
    /// * `side` - ModSides enum value (0 = Client, 1 = Server)
    ///
    /// # Returns
    /// The absolute file path to the mod package ZIP, or null if not found
    #[qjs(rename = "get_mod_package_file_path")]
    pub fn get_mod_package_file_path(&self, mod_id: String, side: u32) -> rquickjs::Result<Option<String>> {
        let mod_side = match ModSide::from_u32(side) {
            Some(s) => s,
            None => {
                tracing::error!("Invalid ModSide value: {}", side);
                return Err(rquickjs::Error::Exception);
            }
        };

        let path = self.system_api.get_mod_package_file_path(&mod_id, mod_side);
        Ok(path.map(|p| p.to_string_lossy().to_string()))
    }

    /// Install a mod from a ZIP file (async)
    ///
    /// Extracts the ZIP file contents to the mods directory under the specified mod_id.
    /// If the mod directory already exists, it is removed first.
    /// This operation runs in a blocking thread pool to avoid blocking the event loop.
    ///
    /// # Arguments
    /// * `zip_path` - Path to the ZIP file to extract
    /// * `mod_id` - The mod identifier (directory name)
    ///
    /// # Returns
    /// Promise that resolves to the installation path on success, or rejects on failure
    #[qjs(rename = "install_mod_from_path")]
    pub async fn install_mod_from_path(&self, zip_path: String, mod_id: String) -> rquickjs::Result<String> {
        tracing::debug!("SystemJS::install_mod_from_path called: zip_path={}, mod_id={}", zip_path, mod_id);

        let system_api = self.system_api.clone();
        let zip_path_owned = zip_path.clone();
        let mod_id_owned = mod_id.clone();

        // Run the blocking ZIP extraction in a separate thread
        let result = tokio::task::spawn_blocking(move || {
            let path = std::path::Path::new(&zip_path_owned);
            system_api.install_mod_from_zip(path, &mod_id_owned)
        })
        .await
        .map_err(|e| {
            tracing::error!("Task join error for mod '{}': {}", mod_id, e);
            rquickjs::Error::Exception
        })?;

        match result {
            Ok(install_path) => Ok(install_path.to_string_lossy().to_string()),
            Err(e) => {
                tracing::error!("Failed to install mod '{}': {}", mod_id, e);
                Err(rquickjs::Error::Exception)
            }
        }
    }

    /// Attach (load and initialize) a mod at runtime
    ///
    /// This function requests the main loop to load a mod that was previously
    /// installed via `install_mod_from_path`. It:
    /// 1. Reads the mod manifest to find the entry point
    /// 2. Loads the mod into the runtime
    /// 3. Calls `onAttach()` on the mod
    /// 4. Marks the mod as loaded
    ///
    /// # Arguments
    /// * `mod_id` - The mod identifier (directory name)
    ///
    /// # Returns
    /// Promise that resolves on success, or rejects on failure
    #[qjs(rename = "attach_mod")]
    pub async fn attach_mod(&self, mod_id: String) -> rquickjs::Result<()> {
        //tracing::debug!("SystemJS::attach_mod called: mod_id={}", mod_id);

        let result = self.system_api.request_attach_mod(mod_id.clone()).await;

        match result {
            Ok(()) => {
                //tracing::info!("Mod '{}' attached successfully", mod_id);
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to attach mod '{}': {}", mod_id, e);
                Err(rquickjs::Error::Exception)
            }
        }
    }
}

/// Setup system API in the JavaScript context
///
/// Provides system.get_mods() function that returns an array of mod info objects.
/// Each mod info object contains: id, version, name, description, mod_type, priority, bootstrapped
pub fn setup_system_api(ctx: Ctx, system_api: SystemApi) -> Result<(), rquickjs::Error> {
    // Initialize the event handlers map (must be done before any handler registration)
    init_event_handlers_map(&ctx)?;

    // First, define the class in the runtime (required before creating instances)
    rquickjs::Class::<SystemJS>::define(&ctx.globals())?;

    // Create an instance of SystemJS
    let system_obj = rquickjs::Class::<SystemJS>::instance(ctx.clone(), SystemJS { system_api })?;

    // Register it as global 'system' object
    ctx.globals().set("system", system_obj)?;

    // Create SystemEvents enum object
    let system_events = Object::new(ctx.clone())?;
    system_events.set("RequestUri", SystemEvents::RequestUri.to_u32())?;
    ctx.globals().set("SystemEvents", system_events)?;

    // Create RequestUriProtocol enum object
    let request_uri_protocol = Object::new(ctx.clone())?;
    request_uri_protocol.set("All", RequestUriProtocol::All.to_u32())?;
    request_uri_protocol.set("Stam", RequestUriProtocol::Stam.to_u32())?;
    request_uri_protocol.set("Http", RequestUriProtocol::Http.to_u32())?;
    ctx.globals()
        .set("RequestUriProtocol", request_uri_protocol)?;

    // Create ModSides enum object (for filtering mod packages by client/server)
    let mod_sides = Object::new(ctx.clone())?;
    mod_sides.set("Client", ModSide::Client.to_u32())?;
    mod_sides.set("Server", ModSide::Server.to_u32())?;
    ctx.globals().set("ModSides", mod_sides)?;

    Ok(())
}

/// JavaScript Locale API class
///
/// This class is exposed to JavaScript as the `locale` global object.
/// It provides methods to get localized strings with optional arguments.
///
/// The locale lookup is hierarchical:
/// 1. First checks the current mod's locale files (if present)
/// 2. Falls back to the global application locale
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct LocaleJS {
    #[qjs(skip_trace)]
    locale_api: LocaleApi,
}

#[rquickjs::methods]
impl LocaleJS {
    /// Get a localized message by ID
    ///
    /// First checks the current mod's locale, then falls back to global locale.
    ///
    /// # Arguments
    /// * `id` - The message ID to look up
    ///
    /// # Returns
    /// The localized string, or `[id]` if not found
    #[qjs(rename = "get")]
    pub fn get(&self, ctx: Ctx<'_>, id: String) -> String {
        // Get the current mod_id from context globals
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());

        self.locale_api.get(&mod_id, &id)
    }

    /// Get a localized message with arguments
    ///
    /// First checks the current mod's locale, then falls back to global locale.
    ///
    /// # Arguments
    /// * `id` - The message ID to look up
    /// * `args` - An object with key-value pairs for substitution
    ///
    /// # Returns
    /// The localized string with arguments substituted, or `[id]` if not found
    #[qjs(rename = "get_with_args")]
    pub fn get_with_args(
        &self,
        ctx: Ctx<'_>,
        id: String,
        args: Object<'_>,
    ) -> rquickjs::Result<String> {
        // Get the current mod_id from context globals
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());

        // Convert JavaScript object to HashMap<String, String>
        let mut args_map = HashMap::new();

        // Iterate over object properties
        for result in args.props::<String, String>() {
            if let Ok((key, value)) = result {
                args_map.insert(key, value);
            }
        }

        Ok(self.locale_api.get_with_args(&mod_id, &id, &args_map))
    }
}

/// Setup locale API in the JavaScript context
///
/// Provides locale.get(id) and locale.get_with_args(id, args) functions
/// for internationalization support in mods.
pub fn setup_locale_api(ctx: Ctx, locale_api: LocaleApi) -> Result<(), rquickjs::Error> {
    // First, define the class in the runtime (required before creating instances)
    rquickjs::Class::<LocaleJS>::define(&ctx.globals())?;

    // Create an instance of LocaleJS
    let locale_obj = rquickjs::Class::<LocaleJS>::instance(ctx.clone(), LocaleJS { locale_api })?;

    // Register it as global 'locale' object
    ctx.globals().set("locale", locale_obj)?;

    Ok(())
}

/// JavaScript Network API class
///
/// This class is exposed to JavaScript as the `network` global object.
/// It provides methods for network operations like downloading resources.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct NetworkJS {
    #[qjs(skip_trace)]
    network_api: NetworkApi,
    #[qjs(skip_trace)]
    temp_file_manager: TempFileManager,
}

#[rquickjs::methods]
impl NetworkJS {
    /// Download a resource from a URI
    ///
    /// # Arguments
    /// * `uri` - The URI to download from (stam://, http://, https://)
    ///
    /// # Returns
    /// A Promise that resolves to an object with:
    /// - status: HTTP status code (u16)
    /// - buffer: Uint8Array | null
    /// - file_name: string | null
    /// - temp_file_path: string | null (path to temp file containing downloaded content)
    #[qjs(rename = "download")]
    pub async fn download<'js>(&self, ctx: Ctx<'js>, uri: String) -> rquickjs::Result<Object<'js>> {
        //tracing::debug!("NetworkJS::download called: uri={}", uri);

        // Perform the download
        let response = self.network_api.download(&uri).await;

        // tracing::debug!("NetworkJS::download response: status={}, buffer_len={:?}, file_name={:?}, file_content_len={:?}, temp_file_path={:?}",
        //     response.status,
        //     response.buffer.as_ref().map(|b| b.len()),
        //     response.file_name,
        //     response.file_content.as_ref().map(|b| b.len()),
        //     response.temp_file_path);

        // Create response object
        let result = Object::new(ctx.clone())?;
        result.set("status", response.status)?;

        // Convert buffer to Uint8Array or null
        if let Some(buffer) = response.buffer {
            let array = rquickjs::TypedArray::<u8>::new(ctx.clone(), buffer)?;
            result.set("buffer", array)?;
        } else {
            result.set("buffer", rquickjs::Null)?;
        }

        // Set file_name
        let file_name_for_temp = response.file_name.clone();
        if let Some(file_name) = response.file_name {
            result.set("file_name", file_name)?;
        } else {
            result.set("file_name", rquickjs::Null)?;
        }

        // If file_content is present, save it to a temp file and return temp_file_path
        // Do NOT expose file_content directly to JavaScript
        if let Some(file_content) = response.file_content {
            //tracing::debug!("NetworkJS::download: file_content has {} bytes, temp_dir={:?}",
            //    file_content.len(), self.temp_file_manager.get_temp_dir());
            match self.temp_file_manager.create_temp_file(&file_content, file_name_for_temp.as_deref()) {
                Ok(temp_path) => {
                    let path_str = temp_path.to_string_lossy().to_string();
                    //tracing::debug!("NetworkJS::download: created temp file at {}", path_str);
                    result.set("temp_file_path", path_str)?;
                }
                Err(e) => {
                    //tracing::error!("Failed to create temp file for download: {}", e);
                    result.set("temp_file_path", rquickjs::Null)?;
                }
            }
        } else {
            result.set("temp_file_path", rquickjs::Null)?;
        }

        Ok(result)
    }
}

/// Setup network API in the JavaScript context
///
/// Provides network.download(uri) function that returns a Promise
/// for downloading resources via stam:// protocol.
///
/// The `temp_file_manager` is used to create temp files for downloaded content
/// and should be cleaned up when the runtime/script finishes.
pub fn setup_network_api(ctx: Ctx, network_api: NetworkApi, temp_file_manager: TempFileManager) -> Result<(), rquickjs::Error> {
    // First, define the class in the runtime (required before creating instances)
    rquickjs::Class::<NetworkJS>::define(&ctx.globals())?;

    // Create an instance of NetworkJS
    let network_obj = rquickjs::Class::<NetworkJS>::instance(ctx.clone(), NetworkJS { network_api, temp_file_manager })?;

    // Register it as global 'network' object
    ctx.globals().set("network", network_obj)?;

    Ok(())
}

/// Setup Text API in the JavaScript context
///
/// Provides Text.DecodeUTF8(u8array) function that decodes a Uint8Array to a UTF-8 string.
pub fn setup_text_api(ctx: Ctx) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    // Create Text object
    let text_obj = Object::new(ctx.clone())?;

    // Text.DecodeUTF8(u8array) -> string
    // Accepts a Uint8Array (or any array-like with numeric values) and returns a UTF-8 decoded string
    let decode_utf8_fn = Function::new(ctx.clone(), |_ctx: Ctx, input: rquickjs::Value| -> rquickjs::Result<String> {
        // Try to get bytes from the input
        let bytes: Vec<u8> = if let Some(typed_array) = input.as_object().and_then(|o| rquickjs::TypedArray::<u8>::from_object(o.clone()).ok()) {
            // It's a Uint8Array
            typed_array.as_bytes().map(|b| b.to_vec()).unwrap_or_default()
        } else if let Some(array) = input.as_array() {
            // It's a regular Array - convert elements to u8
            let mut bytes = Vec::new();
            for i in 0..array.len() {
                let val: i32 = array.get(i)?;
                bytes.push(val as u8);
            }
            bytes
        } else {
            tracing::error!("Text.DecodeUTF8: expected Uint8Array or Array, got {:?}", input.type_of());
            return Err(rquickjs::Error::Exception);
        };

        // Decode as UTF-8
        match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(e) => {
                // Try lossy conversion if strict UTF-8 fails
                tracing::warn!("Text.DecodeUTF8: invalid UTF-8 sequence, using lossy conversion: {}", e);
                Ok(String::from_utf8_lossy(e.as_bytes()).to_string())
            }
        }
    })?;
    text_obj.set("DecodeUTF8", decode_utf8_fn)?;

    // Register Text object globally
    globals.set("Text", text_obj)?;

    Ok(())
}
