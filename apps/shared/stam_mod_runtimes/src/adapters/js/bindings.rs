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

use crate::api::{AppApi, ConsoleApi, FileApi, LocaleApi, NetworkApi, ReadJsonResult, RequestUriProtocol, SystemApi, SystemEvents, ModSide};
use crate::api::path_security::{validate_path_for_creation, ModPathConfig, resolve_mod_path};

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

/// Name of the global JavaScript object used to store widget event callbacks
/// Key format: "widgetId:eventType", value: callback function
const JS_WIDGET_HANDLERS_MAP: &str = "__widgetHandlers";

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

/// Initialize the widget event handlers map in a JavaScript context
pub fn init_widget_handlers_map(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let handlers_map = Object::new(ctx.clone())?;
    ctx.globals().set(JS_WIDGET_HANDLERS_MAP, handlers_map)?;
    Ok(())
}

/// Store a widget event callback in the context's widget handlers map
///
/// # Arguments
/// * `ctx` - JavaScript context
/// * `widget_id` - Widget ID
/// * `event_type` - Event type ("click", "hover", "focus")
/// * `callback` - JavaScript callback function
pub fn store_widget_handler<'js>(
    ctx: &Ctx<'js>,
    widget_id: u64,
    event_type: &str,
    callback: Function<'js>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let handlers_map: Object = globals.get(JS_WIDGET_HANDLERS_MAP)?;
    let key = format!("{}:{}", widget_id, event_type);
    handlers_map.set(key, callback)?;
    Ok(())
}

/// Remove a widget event callback from the context's widget handlers map
///
/// Returns true if a handler was removed, false if none was found
pub fn remove_widget_handler(ctx: &Ctx<'_>, widget_id: u64, event_type: &str) -> rquickjs::Result<bool> {
    let globals = ctx.globals();
    let handlers_map: Object = globals.get(JS_WIDGET_HANDLERS_MAP)?;
    let key = format!("{}:{}", widget_id, event_type);
    let exists: bool = handlers_map.contains_key(&key)?;
    if exists {
        handlers_map.set(&key, rquickjs::Undefined)?;
    }
    Ok(exists)
}

/// Remove all event handlers for a widget
///
/// Removes handlers for all event types (click, hover, focus)
pub fn remove_all_widget_handlers(ctx: &Ctx<'_>, widget_id: u64) -> rquickjs::Result<()> {
    // Remove handlers for all known event types
    let event_types = ["click", "hover", "focus"];
    for event_type in event_types {
        let _ = remove_widget_handler(ctx, widget_id, event_type)?;
    }
    Ok(())
}

/// Get a widget event callback from the context's widget handlers map
pub fn get_widget_handler<'js>(
    ctx: &Ctx<'js>,
    widget_id: u64,
    event_type: &str,
) -> rquickjs::Result<Option<Function<'js>>> {
    let globals = ctx.globals();
    let handlers_map: Object = globals.get(JS_WIDGET_HANDLERS_MAP)?;
    let key = format!("{}:{}", widget_id, event_type);
    let handler: rquickjs::Value = handlers_map.get(&key)?;
    if handler.is_undefined() || handler.is_null() {
        Ok(None)
    } else {
        Ok(handler.into_function())
    }
}

/// Setup console API in the JavaScript context
///
/// Provides console.log, console.error, console.warn, console.info, console.debug, console.trace
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

    // Register Process object globally (capitalized for Staminal convention)
    globals.set("Process", process)?;

    Ok(())
}

/// JavaScript File API class
///
/// This class is exposed to JavaScript as the `File` global object.
/// It provides methods to read and write files with path security validation.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct FileJS {
    #[qjs(skip_trace)]
    file_api: FileApi,
}

#[rquickjs::methods]
impl FileJS {
    /// Read a JSON file and parse it
    ///
    /// # Arguments
    /// * `path` - Path to the JSON file (relative or absolute)
    /// * `encoding` - File encoding (only "utf-8" supported)
    /// * `default_value` - Default value to return if file doesn't exist or is empty
    ///                     Must be an object, array, null, or undefined. Primitives are rejected.
    ///
    /// # Returns
    /// The parsed JSON object, or the default value if file doesn't exist/is empty
    ///
    /// # Throws
    /// - Error if path escapes permitted directories (path traversal)
    /// - Error if file contains invalid JSON
    /// - Error if default_value is a primitive (string, number, boolean)
    /// - Error if encoding is not "utf-8"
    ///
    /// # Example
    /// ```javascript
    /// // Read config, return empty object if file doesn't exist
    /// const config = await File.readJson("settings.json", "utf-8", {});
    ///
    /// // Read with default config values
    /// const config = await File.readJson("settings.json", "utf-8", { volume: 50, fullscreen: false });
    /// ```
    #[qjs(rename = "readJson")]
    pub fn read_json<'js>(
        &self,
        ctx: Ctx<'js>,
        path: String,
        encoding: String,
        default_value: Opt<Value<'js>>,
    ) -> rquickjs::Result<Value<'js>> {
        // Determine default value
        let default_val = default_value.0;

        // Validate default_value is not a primitive
        if let Some(ref val) = default_val {
            if !val.is_undefined() && !val.is_null() && !val.is_object() && !val.is_array() {
                return Err(ctx.throw(rquickjs::String::from_str(
                    ctx.clone(),
                    "File.readJson() default_value must be an object, array, null, or undefined. Primitive types (string, number, boolean) are not allowed.",
                )?.into()));
            }
        }

        // Call the Rust API
        match self.file_api.read_json(&path, &encoding) {
            ReadJsonResult::Success(json_str) => {
                // Parse JSON string into JavaScript value
                let json_global: Object = ctx.globals().get("JSON")?;
                let parse_fn: Function = json_global.get("parse")?;
                parse_fn.call((json_str,))
            }
            ReadJsonResult::UseDefault => {
                // Return default value or empty object
                match default_val {
                    Some(val) => Ok(val),
                    None => {
                        // Return empty object if no default provided
                        let obj = Object::new(ctx)?;
                        Ok(obj.into_value())
                    }
                }
            }
            ReadJsonResult::Error(msg) => {
                Err(ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg)?.into()))
            }
        }
    }
}

/// Setup File API in the JavaScript context
///
/// Provides File.readJson() for secure file operations with path validation.
///
/// # Arguments
/// * `ctx` - The JavaScript context
/// * `file_api` - The FileApi instance with configured data_dir and config_dir
pub fn setup_file_api(ctx: Ctx, file_api: FileApi) -> Result<(), rquickjs::Error> {
    // Define the FileJS class
    rquickjs::Class::<FileJS>::define(&ctx.globals())?;

    // Create an instance of FileJS
    let file_obj = rquickjs::Class::<FileJS>::instance(ctx.clone(), FileJS { file_api })?;

    // Register it as global 'File' object (capitalized for Staminal convention)
    ctx.globals().set("File", file_obj)?;

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

    // Timer scheduled - logging disabled for now
    // let timer_type = if is_interval { "setInterval" } else { "setTimeout" };
    // tracing::trace!("{}: timer {} scheduled with {}ms delay for mod '{}'", timer_type, id, delay, mod_id);

    // Spawn async task in the JS context
    ctx.spawn(async move {
        let duration = tokio::time::Duration::from_millis(delay);

        loop {
            tokio::select! {
                biased;

                // Check for cancellation
                _ = abort_ref.notified() => {
                    tracing::trace!("Timer {} aborted", id);
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
        tracing::trace!("clearTimeout: cancelling timer {}", timer_id);
        clear_timer(timer_id);
    })?;
    globals.set("clearTimeout", clear_timeout_fn)?;

    // clearInterval(intervalId) - cancels a pending interval
    let clear_interval_fn = Function::new(ctx.clone(), |_ctx: Ctx, timer_id: u32| {
        tracing::trace!("clearInterval: cancelling interval {}", timer_id);
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
    /// Game config directory (optional, client-only)
    /// Used by getGameConfigPath() to resolve config file paths
    #[qjs(skip_trace)]
    game_config_dir: Option<PathBuf>,
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
    /// - loaded: boolean
    /// - exists: boolean
    /// - download_url: string | null
    /// - archive_sha512: string | null (if available from server)
    /// - archive_bytes: number | null (if available from server)
    /// - uncompressed_bytes: number | null (if available from server)
    #[qjs(rename = "getMods")]
    pub fn get_mods<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Array<'js>> {
        tracing::trace!("SystemJS::get_mods called");

        let mods = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.system_api.get_mods()
        })) {
            Ok(mods) => mods,
            Err(e) => {
                //tracing::error!("Panic in get_mods: {:?}", e);
                return Err(rquickjs::Error::Exception);
            }
        };

        tracing::trace!("SystemJS::get_mods got {} mods", mods.len());

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
            obj.set("archive_sha512", mod_info.archive_sha512.as_deref())?;
            if let Some(bytes) = mod_info.archive_bytes {
                obj.set("archive_bytes", bytes)?;
            } else {
                obj.set("archive_bytes", rquickjs::Null)?;
            }
            if let Some(bytes) = mod_info.uncompressed_bytes {
                obj.set("uncompressed_bytes", bytes)?;
            } else {
                obj.set("uncompressed_bytes", rquickjs::Null)?;
            }
            array.set(idx, obj)?;
        }

        tracing::trace!("SystemJS::get_mods returning array");
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
    #[qjs(rename = "registerEvent")]
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

            // tracing::debug!(
            //     "SystemJS::register_event called: mod={}, event={}, priority={}, protocol={:?}, route={:?}",
            //     mod_id,
            //     event_u32,
            //     priority,
            //     protocol_str,
            //     route
            // );

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

            // tracing::debug!(
            //     "Registered event handler: mod={}, event={:?}, handler_id={}, priority={}",
            //     mod_id,
            //     event_type,
            //     handler_id,
            //     priority
            // );

            Ok(handler_id)
        } else if let Some(event_name) = event.as_string() {
            // Custom event (string)
            let event_name_str = event_name.to_string()?;

            // tracing::debug!(
            //     "SystemJS::register_event (custom) called: mod={}, event_name={}, priority={}",
            //     mod_id,
            //     event_name_str,
            //     priority
            // );

            // Register the handler with the event dispatcher
            let handler_id = self.system_api.event_dispatcher().register_custom_handler(
                &event_name_str,
                &mod_id,
                priority,
            );

            // Store the handler function in the context's handler map
            store_js_handler(&ctx, handler_id, handler)?;

            // tracing::debug!(
            //     "Registered custom event handler: mod={}, event_name={}, handler_id={}, priority={}",
            //     mod_id,
            //     event_name_str,
            //     handler_id,
            //     priority
            // );

            Ok(handler_id)
        } else {
            tracing::error!("register_event: first argument must be a number (SystemEvents) or string (custom event name)");
            Err(rquickjs::Error::Exception)
        }
    }

    /// Send a custom event to all registered handlers
    ///
    /// This function triggers all handlers registered for the given event name,
    /// passing the provided arguments to each handler. The dispatch happens through
    /// the main event loop which has access to all mod contexts.
    ///
    /// **IMPORTANT**: Handler response values (like `res.handled = true`) must be set
    /// SYNCHRONOUSLY, before any `await` points. Values set after an `await` will not
    /// be captured in the response.
    ///
    /// # Arguments
    /// * `event_name` - The custom event name to dispatch
    /// * `args` - Variadic arguments to pass to handlers (will be JSON-serialized)
    ///
    /// # Returns
    /// Promise that resolves to an object with:
    /// - `handled: boolean` - Whether any handler marked the event as handled
    /// - Plus any custom properties added by handlers
    ///
    /// # Example
    /// ```javascript
    /// const result = await system.sendEvent("AppStart", { data: "test" });
    /// if (result.handled) {
    ///     console.log("Event was handled");
    /// }
    /// ```
    #[qjs(rename = "sendEvent")]
    pub async fn send_event<'js>(&self, ctx: Ctx<'js>, event_name: String, args: Rest<Value<'js>>) -> rquickjs::Result<Object<'js>> {
        // Convert each JS value to JSON string
        let json_args: Vec<String> = args.0.iter()
            .map(|v| ctx.json_stringify(v.clone())
                .ok()
                .flatten()
                .map(|s| s.to_string().unwrap_or_default())
                .unwrap_or_else(|| "null".to_string()))
            .collect();

        tracing::trace!("SystemJS::send_event called: event_name={}, args_count={}", event_name, json_args.len());

        let result = self.system_api.event_dispatcher().request_send_event(event_name.clone(), json_args).await;

        match result {
            Ok(response) => {
                tracing::trace!("Event '{}' dispatched successfully (handled={}, properties={})",
                    event_name, response.handled, response.properties.len());

                // Create response object
                let response_obj = Object::new(ctx.clone())?;
                response_obj.set("handled", response.handled)?;

                // Add all custom properties from handlers
                for (key, value_json) in response.properties.iter() {
                    let js_value: Value = ctx.json_parse(value_json.clone())
                        .unwrap_or_else(|_| Value::new_null(ctx.clone()));
                    response_obj.set(key.as_str(), js_value)?;
                }

                Ok(response_obj)
            }
            Err(e) => {
                tracing::error!("Failed to dispatch event '{}': {}", event_name, e);
                Err(ctx.throw(rquickjs::String::from_str(ctx.clone(), &e)?.into()))
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
    #[qjs(rename = "unregisterEvent")]
    pub fn unregister_event(&self, ctx: Ctx<'_>, handler_id: u64) -> rquickjs::Result<bool> {
        // tracing::debug!(
        //     "SystemJS::unregister_event called: handler_id={}",
        //     handler_id
        // );

        // Remove from event dispatcher
        let removed = self
            .system_api
            .event_dispatcher()
            .unregister_handler(handler_id);

        // Remove the handler function from the context's map
        if removed {
            remove_js_handler(&ctx, handler_id)?;
            tracing::debug!("Unregistered event handler: handler_id={}", handler_id);
        }

        Ok(removed)
    }

    /// Exit the application immediately with the specified exit code
    ///
    /// # Arguments
    /// * `code` - The exit code (0 = success, non-zero = error)
    ///
    /// # Note
    /// This function requests a graceful shutdown instead of terminating immediately.
    /// The main loop will receive the shutdown request and perform cleanup before exiting.
    #[qjs(rename = "exit")]
    pub fn exit(&self, code: i32) {
        tracing::debug!("SystemJS::exit called with code {} - requesting graceful shutdown", code);
        if let Err(e) = self.system_api.request_shutdown(code) {
            tracing::error!("Failed to request shutdown: {}", e);
            // Fallback to immediate exit if channel is not available
            std::process::exit(code);
        }
    }

    /// Terminate the application immediately with the specified exit code
    ///
    /// # Arguments
    /// * `code` - The exit code (0 = success, non-zero = error)
    ///
    /// # Note
    /// This function terminates the process immediately without cleanup.
    /// Use `exit()` for graceful shutdown. Only use `terminate()` when
    /// immediate termination is required (e.g., fatal errors).
    #[qjs(rename = "terminate")]
    pub fn terminate(&self, code: i32) {
        tracing::warn!("SystemJS::terminate called with code {} - immediate termination", code);
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
    /// - archive_sha512: string (SHA512 hash of archive for integrity verification)
    /// - archive_bytes: number (size of compressed archive in bytes)
    /// - uncompressed_bytes: number (sum of all uncompressed file sizes in bytes)
    /// - path: string
    /// - manifest: object with name, version, description, entry_point, etc.
    #[qjs(rename = "getModPackages")]
    pub fn get_mod_packages<'js>(&self, ctx: Ctx<'js>, side: u32) -> rquickjs::Result<Array<'js>> {
        let mod_side = match ModSide::from_u32(side) {
            Some(s) => s,
            None => {
                tracing::error!("Invalid ModSide value: {}", side);
                return Err(rquickjs::Error::Exception);
            }
        };

        let packages = self.system_api.get_mod_packages(mod_side);
        tracing::trace!("SystemJS::get_mod_packages called: side={:?}, found {} packages", mod_side, packages.len());

        let array = Array::new(ctx.clone())?;

        for (idx, pkg) in packages.iter().enumerate() {
            let obj = Object::new(ctx.clone())?;
            obj.set("id", pkg.id.as_str())?;
            obj.set("archive_sha512", pkg.archive_sha512.as_str())?;
            obj.set("archive_bytes", pkg.archive_bytes)?;
            obj.set("uncompressed_bytes", pkg.uncompressed_bytes)?;
            obj.set("path", pkg.path.as_str())?;

            // Create manifest object
            let manifest_obj = Object::new(ctx.clone())?;
            manifest_obj.set("name", pkg.manifest.name.as_str())?;
            manifest_obj.set("version", pkg.manifest.version.as_str())?;
            manifest_obj.set("description", pkg.manifest.description.as_str())?;
            manifest_obj.set("entry_point", pkg.manifest.entry_point.as_deref())?;
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
    #[qjs(rename = "getModPackageFilePath")]
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

    /// Install a mod from a tar.gz archive (async)
    ///
    /// Extracts the tar.gz archive contents to the mods directory under the specified mod_id.
    /// If the mod directory already exists, it is removed first.
    /// This operation runs in a blocking thread pool to avoid blocking the event loop.
    ///
    /// # Arguments
    /// * `archive_path` - Path to the tar.gz file to extract
    /// * `mod_id` - The mod identifier (directory name)
    ///
    /// # Returns
    /// Promise that resolves to the installation path on success, or rejects on failure
    #[qjs(rename = "installModFromPath")]
    pub async fn install_mod_from_path(&self, archive_path: String, mod_id: String) -> rquickjs::Result<String> {
        tracing::trace!("SystemJS::install_mod_from_path called: archive_path={}, mod_id={}", archive_path, mod_id);

        let system_api = self.system_api.clone();
        let archive_path_owned = archive_path.clone();
        let mod_id_owned = mod_id.clone();

        // Run the blocking tar.gz extraction in a separate thread
        let result = tokio::task::spawn_blocking(move || {
            let path = std::path::Path::new(&archive_path_owned);
            system_api.install_mod_from_archive(path, &mod_id_owned)
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
    #[qjs(rename = "attachMod")]
    pub async fn attach_mod(&self, mod_id: String) -> rquickjs::Result<()> {
        tracing::trace!("SystemJS::attach_mod called: mod_id={}", mod_id);

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

    /// Get information about the current game context (client-side only)
    ///
    /// # Returns
    /// An object with:
    /// - id: string - The game identifier
    /// - name: string - The game display name
    /// - version: string - The game version
    ///
    /// # Throws
    /// Error if called on the server (game info not available on server)
    #[qjs(rename = "getGameInfo")]
    pub fn get_game_info<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Object<'js>> {
        match self.system_api.get_game_info() {
            Some(game_info) => {
                let obj = Object::new(ctx)?;
                obj.set("id", game_info.id.as_str())?;
                obj.set("name", game_info.name.as_str())?;
                obj.set("version", game_info.version.as_str())?;
                Ok(obj)
            }
            None => {
                // Throw a descriptive error for server-side calls
                Err(ctx.throw(rquickjs::String::from_str(
                    ctx.clone(),
                    "system.get_game_info() is not available on the server. This method is client-only.",
                )?
                .into()))
            }
        }
    }

    /// Get the full path for a config file within the game config directory (client-only)
    ///
    /// This method takes a relative path and returns the full absolute path within
    /// the game's config directory. The path is validated to ensure it doesn't escape
    /// the config directory (path traversal attacks like `../` are blocked).
    ///
    /// # Arguments
    /// * `relative_path` - The relative path within the config directory (e.g., "settings.json", "saves/game1.json")
    ///
    /// # Returns
    /// The full absolute path to the config file
    ///
    /// # Throws
    /// - Error if called on the server (config directory not available)
    /// - Error if the path attempts to escape the config directory (path traversal)
    /// - Error if the relative path is absolute
    ///
    /// # Security
    /// This method validates the path to prevent directory traversal attacks:
    /// - Absolute paths are rejected
    /// - Paths containing `..` that would escape the config directory are rejected
    /// - The file does not need to exist (useful for creating new config files)
    ///
    /// # Examples
    /// ```javascript
    /// // Get path for a config file
    /// const settingsPath = system.getGameConfigPath("settings.json");
    /// // Returns: "/home/user/.config/game/settings.json"
    ///
    /// // Get path for a nested config file
    /// const savePath = system.getGameConfigPath("saves/slot1.json");
    /// // Returns: "/home/user/.config/game/saves/slot1.json"
    ///
    /// // This will throw an error (path traversal attempt)
    /// system.getGameConfigPath("../../../etc/passwd"); // Error!
    /// ```
    #[qjs(rename = "getGameConfigPath")]
    pub fn get_game_config_path<'js>(&self, ctx: Ctx<'js>, relative_path: String) -> rquickjs::Result<String> {
        // Check if game_config_dir is available (client-only)
        match &self.game_config_dir {
            Some(config_dir) => {
                // Validate the path to prevent directory traversal
                match validate_path_for_creation(&relative_path, config_dir) {
                    Ok(full_path) => Ok(full_path.to_string_lossy().to_string()),
                    Err(error_msg) => {
                        Err(ctx.throw(rquickjs::String::from_str(ctx.clone(), &error_msg)?.into()))
                    }
                }
            }
            None => {
                // Throw a descriptive error for server-side calls
                Err(ctx.throw(rquickjs::String::from_str(
                    ctx.clone(),
                    "system.getGameConfigPath() is not available on the server. This method is client-only.",
                )?
                .into()))
            }
        }
    }

    /// Resolve an asset path for the current mod
    ///
    /// This method resolves relative asset paths to actual file paths.
    /// The path resolution follows these rules:
    ///
    /// 1. If the path starts with "@modid/", it looks in that mod's assets folder.
    ///    If the mod doesn't exist, an error is thrown.
    /// 2. Otherwise, it first checks the current mod's assets folder.
    /// 3. If not found there, it checks the client's global assets directory.
    ///
    /// # Arguments
    /// * `relative_path` - The relative asset path (e.g., "fonts/MyFont.ttf" or "@other-mod/icons/icon.png")
    ///
    /// # Returns
    /// The resolved path relative to the client's data root (e.g., "mods/my-mod/assets/fonts/MyFont.ttf")
    ///
    /// # Throws
    /// Error if the asset is not found or if the referenced mod doesn't exist
    ///
    /// # Examples
    /// ```javascript
    /// // Look in current mod's assets folder, fallback to global assets
    /// const fontPath = system.getAssetsPath("fonts/PerfectDOSVGA437.ttf");
    ///
    /// // Look in another mod's assets folder
    /// const iconPath = system.getAssetsPath("@ui-toolkit/icons/close.png");
    /// ```
    #[qjs(rename = "getAssetsPath")]
    pub fn get_assets_path<'js>(&self, ctx: Ctx<'js>, relative_path: String) -> rquickjs::Result<String> {
        // Get the current mod_id from context globals
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());

        match self.system_api.get_assets_path(&mod_id, &relative_path) {
            Ok(resolved_path) => Ok(resolved_path),
            Err(error_msg) => {
                Err(ctx.throw(rquickjs::String::from_str(ctx.clone(), &error_msg)?.into()))
            }
        }
    }
}

/// Setup system API in the JavaScript context
///
/// Provides system.get_mods() function that returns an array of mod info objects.
/// Each mod info object contains: id, version, name, description, mod_type, priority, bootstrapped
///
/// # Arguments
/// * `ctx` - The JavaScript context
/// * `system_api` - The system API instance
/// * `game_config_dir` - Optional game config directory (client-only, for getGameConfigPath)
pub fn setup_system_api(ctx: Ctx, system_api: SystemApi, game_config_dir: Option<PathBuf>) -> Result<(), rquickjs::Error> {
    // Initialize the event handlers map (must be done before any handler registration)
    init_event_handlers_map(&ctx)?;
    init_widget_handlers_map(&ctx)?;

    // First, define the class in the runtime (required before creating instances)
    rquickjs::Class::<SystemJS>::define(&ctx.globals())?;

    // Create an instance of SystemJS
    let system_obj = rquickjs::Class::<SystemJS>::instance(ctx.clone(), SystemJS { system_api, game_config_dir })?;

    // Register it as global 'System' object (capitalized for Staminal convention)
    ctx.globals().set("System", system_obj)?;

    // Create SystemEvents enum object
    let system_events = Object::new(ctx.clone())?;
    system_events.set("RequestUri", SystemEvents::RequestUri.to_u32())?;
    system_events.set("TerminalKeyPressed", SystemEvents::TerminalKeyPressed.to_u32())?;
    system_events.set("GraphicEngineReady", SystemEvents::GraphicEngineReady.to_u32())?;
    system_events.set("GraphicEngineWindowClosed", SystemEvents::GraphicEngineWindowClosed.to_u32())?;
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
    #[qjs(rename = "getWithArgs")]
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
        // Handle any JS value type (string, number, boolean, etc.) by converting to string
        let mut args_map = HashMap::new();

        // Iterate over object properties with Value type to handle any JS type
        for result in args.props::<String, rquickjs::Value>() {
            if let Ok((key, value)) = result {
                // Convert JS value to string representation
                let string_value = if value.is_string() {
                    value.as_string().map(|s| s.to_string().unwrap_or_default()).unwrap_or_default()
                } else if value.is_int() {
                    value.as_int().map(|n| n.to_string()).unwrap_or_default()
                } else if value.is_float() {
                    value.as_float().map(|n| n.to_string()).unwrap_or_default()
                } else if value.is_bool() {
                    value.as_bool().map(|b| b.to_string()).unwrap_or_default()
                } else {
                    // Fallback: try to convert to string via coercion
                    value.as_string().map(|s| s.to_string().unwrap_or_default()).unwrap_or_default()
                };
                args_map.insert(key, string_value);
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

    // Register it as global 'Locale' object (capitalized for Staminal convention)
    ctx.globals().set("Locale", locale_obj)?;

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
    /// Download a resource from a URI with optional progress callback
    ///
    /// # Arguments
    /// * `uri` - The URI to download from (stam://, http://, https://)
    /// * `progress_callback` - Optional callback function(percentage, receivedBytes, totalBytes)
    ///
    /// # Returns
    /// A Promise that resolves to an object with:
    /// - status: HTTP status code (u16)
    /// - buffer: Uint8Array | null
    /// - file_name: string | null
    /// - temp_file_path: string | null (path to temp file containing downloaded content)
    ///
    /// # Example
    /// ```javascript
    /// const response = await network.download(uri, (percentage, receivedBytes, totalBytes) => {
    ///     console.log(`Progress: ${percentage}% (${receivedBytes}/${totalBytes})`);
    /// });
    /// ```
    #[qjs(rename = "download")]
    pub async fn download<'js>(&self, ctx: Ctx<'js>, uri: String, progress_callback: Opt<Function<'js>>) -> rquickjs::Result<Object<'js>> {
        tracing::trace!("NetworkJS::download called: uri={}", uri);

        // Shared state for progress updates: (percentage, received, total)
        // The Rust progress callback updates this, and we read it periodically to call JS
        let progress_state = Arc::new(std::sync::Mutex::new((0.0f64, 0u64, 0u64)));
        let state_for_callback = progress_state.clone();

        // Create Rust progress callback that updates shared state
        let progress_cb: Option<crate::api::ProgressCallback> = if progress_callback.0.is_some() {
            Some(Arc::new(move |percentage: f64, received: u64, total: u64| {
                if let Ok(mut state) = state_for_callback.lock() {
                    *state = (percentage, received, total);
                }
            }) as crate::api::ProgressCallback)
        } else {
            None
        };

        // Get the JS callback if provided
        let js_callback = progress_callback.0;

        // If we have a JS callback, set up periodic progress reporting
        let response = if let Some(ref callback) = js_callback {
            let state_for_polling = progress_state.clone();
            let callback_clone = callback.clone();
            let ctx_clone = ctx.clone();

            // Use tokio::select to run download and periodic callback together
            let download_future = self.network_api.download_with_progress(&uri, progress_cb);

            // We'll poll the state every second while download is in progress
            let mut last_reported = (0.0f64, 0u64, 0u64);

            tokio::pin!(download_future);

            loop {
                tokio::select! {
                    result = &mut download_future => {
                        // Download completed - break and return result
                        break result;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => {
                        // Timer fired - check progress and call JS callback if changed
                        let current = {
                            // Scope the lock to release it quickly
                            if let Ok(state) = state_for_polling.lock() {
                                *state
                            } else {
                                continue;
                            }
                        };
                        // Only call if there's been progress
                        if current.1 > last_reported.1 {
                            last_reported = current;
                            // Call the JS callback with current progress
                            let _ = callback_clone.call::<_, ()>((current.0, current.1, current.2));
                            // Yield to allow JS runtime to process the callback
                            //tokio::task::yield_now().await;
                        }
                    }
                }
            }
        } else {
            // No callback, just do the download
            self.network_api.download_with_progress(&uri, progress_cb).await
        };

        // Call the JS callback one final time with 100% (or current state if failed)
        if let Some(ref callback) = js_callback {
            let final_state = progress_state.lock().map(|s| *s).unwrap_or((100.0, 0, 0));
            let final_percentage = if response.status == 200 { 100.0 } else { final_state.0 };
            let _ = callback.call::<_, ()>((final_percentage, final_state.1, final_state.2));
        }

        // Create response object
        let result = Object::new(ctx.clone())?;
        result.set("status", response.status)?;

        // Set buffer_string (or null)
        if let Some(buffer_str) = response.buffer_string {
            result.set("bufferString", buffer_str)?;
        } else {
            result.set("bufferString", rquickjs::Null)?;
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
            tracing::trace!("NetworkJS::download: file_content has {} bytes", file_content.len());
            match self.temp_file_manager.create_temp_file(&file_content, file_name_for_temp.as_deref()) {
                Ok(temp_path) => {
                    let path_str = temp_path.to_string_lossy().to_string();
                    tracing::trace!("NetworkJS::download: created temp file at {}", path_str);
                    result.set("temp_file_path", path_str)?;
                }
                Err(_e) => {
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

    // Register it as global 'Network' object (capitalized for Staminal convention)
    ctx.globals().set("Network", network_obj)?;

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

// ============================================================================
// Graphic API Bindings
// ============================================================================

use crate::api::{
    AlignItems, ColorValue, EdgeInsets, FlexDirection, FontConfig, GraphicEngines, GraphicProxy,
    InitialWindowConfig, JustifyContent, PropertyValue, SizeValue, WidgetConfig, WidgetEventType,
    WidgetType, WindowConfig, WindowMode, WindowPositionMode,
};

/// JavaScript Graphic API class
///
/// Exposed as the `graphic` global object in JavaScript.
/// Provides methods to enable graphic engines and create windows.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct GraphicJS {
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
}

#[rquickjs::methods]
impl GraphicJS {
    /// Enable a graphic engine
    ///
    /// # Arguments
    /// * `engine_type` - GraphicEngines enum value (0 = Bevy, 1 = Wgpu, 2 = Terminal)
    /// * `config` - Optional configuration object with:
    ///   - window: Object with initial window settings:
    ///     - title: string (default: "Staminal")
    ///     - width: number (default: 1280)
    ///     - height: number (default: 720)
    ///     - resizable: boolean (default: true)
    ///     - fullscreen: boolean (default: false)
    ///     - positionMode: WindowPositionModes enum value (default: Centered)
    ///
    /// # Returns
    /// Promise that resolves when engine is ready
    ///
    /// # Throws
    /// Error if called on server or if engine is already enabled
    #[qjs(rename = "enableEngine")]
    pub async fn enable_engine<'js>(
        &self,
        ctx: Ctx<'js>,
        engine_type: u32,
        config: Opt<Object<'js>>,
    ) -> rquickjs::Result<()> {
        let engine = GraphicEngines::from_u32(engine_type).ok_or_else(|| {
            let msg = format!("Invalid engine type: {}. Use GraphicEngines.Bevy (0), GraphicEngines.Wgpu (1), or GraphicEngines.Terminal (2)", engine_type);
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
        })?;

        // Parse the optional config object
        let initial_window_config = if let Some(cfg) = config.0 {
            // Try to get the window sub-object
            if let Ok(window_obj) = cfg.get::<_, Object>("window") {
                Some(InitialWindowConfig {
                    title: window_obj.get::<_, String>("title").unwrap_or_else(|_| "Staminal".to_string()),
                    width: window_obj.get::<_, u32>("width").unwrap_or(1280),
                    height: window_obj.get::<_, u32>("height").unwrap_or(720),
                    resizable: window_obj.get::<_, bool>("resizable").unwrap_or(true),
                    fullscreen: window_obj.get::<_, bool>("fullscreen").unwrap_or(false),
                    position_mode: WindowPositionMode::from_u32(
                        window_obj.get::<_, u32>("positionMode").unwrap_or(1) // Default: Centered
                    ),
                })
            } else {
                None
            }
        } else {
            None
        };

        self.graphic_proxy.enable_engine(engine, initial_window_config).await.map_err(|e| {
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into())
        })
    }

    /// Check if a graphic engine is enabled
    ///
    /// # Returns
    /// true if an engine is currently enabled, false otherwise
    #[qjs(rename = "isEngineEnabled")]
    pub fn is_engine_enabled(&self) -> bool {
        self.graphic_proxy.is_engine_enabled()
    }

    /// Get the current engine type
    ///
    /// # Returns
    /// GraphicEngines enum value, or null if no engine is enabled
    #[qjs(rename = "getEngine")]
    pub fn get_engine(&self) -> Option<u32> {
        self.graphic_proxy.get_active_engine().map(|e| e.to_u32())
    }

    /// Set the main window
    ///
    /// Promotes a window to be the "main" window. After this call:
    /// - `Graphic.getEngineInfo().mainWindow` will return this window
    /// - Window close events will reflect the new main window
    ///
    /// This is useful when you create a new window to replace the initial
    /// loading/splash window and want to promote it as the main game window.
    ///
    /// # Arguments
    /// * `window` - The Window object to set as the main window
    ///
    /// # Throws
    /// Error if called on server or if the window is invalid
    ///
    /// # Example
    /// ```javascript
    /// const gameWindow = await Graphic.createWindow({ title: "Game", ... });
    /// Graphic.setMainWindow(gameWindow);
    /// // Now getEngineInfo().mainWindow returns gameWindow
    /// ```
    #[qjs(rename = "setMainWindow")]
    pub fn set_main_window(&self, ctx: Ctx<'_>, window: rquickjs::Class<'_, WindowJS>) -> rquickjs::Result<()> {
        let window_id = window.borrow().id;
        self.graphic_proxy
            .set_main_window(window_id)
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Get detailed information about the active graphic engine
    ///
    /// # Returns
    /// Promise that resolves to an object containing:
    /// - engineType: string (e.g., "Bevy")
    /// - engineTypeId: number (GraphicEngines enum value)
    /// - name: string (library name)
    /// - version: string (library version)
    /// - description: string (engine description)
    /// - features: string[] (list of enabled features)
    /// - backend: string (rendering backend, e.g., "Vulkan")
    /// - supports2d: boolean
    /// - supports3d: boolean
    /// - supportsUi: boolean
    /// - supportsAudio: boolean
    /// - mainWindow: Window (the primary/main window created at engine startup)
    ///
    /// # Throws
    /// Error if called on server or if no engine is enabled
    #[qjs(rename = "getEngineInfo")]
    pub async fn get_engine_info<'js>(
        &self,
        ctx: Ctx<'js>,
    ) -> rquickjs::Result<Object<'js>> {
        let info = self.graphic_proxy.get_engine_info().await.map_err(|e| {
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into())
        })?;

        // Create JavaScript object from GraphicEngineInfo
        let obj = Object::new(ctx.clone())?;
        obj.set("engineType", info.engine_type)?;
        obj.set("engineTypeId", info.engine_type_id)?;
        obj.set("name", info.name)?;
        obj.set("version", info.version)?;
        obj.set("description", info.description)?;

        // Convert features Vec to JS array
        let features_array = rquickjs::Array::new(ctx.clone())?;
        for (i, feature) in info.features.iter().enumerate() {
            features_array.set(i, feature.as_str())?;
        }
        obj.set("features", features_array)?;

        obj.set("backend", info.backend)?;
        obj.set("supports2d", info.supports_2d)?;
        obj.set("supports3d", info.supports_3d)?;
        obj.set("supportsUi", info.supports_ui)?;
        obj.set("supportsAudio", info.supports_audio)?;

        // Create mainWindow object wrapping the current main window
        // The main window ID can be changed via Graphic.setMainWindow()
        let main_window_id = self.graphic_proxy.get_main_window_id();
        let main_window = rquickjs::Class::<WindowJS>::instance(
            ctx.clone(),
            WindowJS {
                id: main_window_id,
                graphic_proxy: self.graphic_proxy.clone(),
            },
        )?;
        obj.set("mainWindow", main_window)?;

        Ok(obj)
    }

    /// Get all windows currently managed by the graphic engine
    ///
    /// # Returns
    /// Array of Window objects that can be operated on (e.g., win.close(), win.setTitle())
    ///
    /// # Throws
    /// Error if called on server or if no engine is enabled
    ///
    /// # Example
    /// ```javascript
    /// const windows = await graphic.getWindows();
    /// for (const win of windows) {
    ///     console.log(`Window ${win.getId()}`);
    /// }
    /// ```
    #[qjs(rename = "getWindows")]
    pub fn get_windows<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<rquickjs::Array<'js>> {
        let window_ids = self.graphic_proxy.get_window_ids();
        let arr = rquickjs::Array::new(ctx.clone())?;
        for (i, id) in window_ids.iter().enumerate() {
            let window = rquickjs::Class::<WindowJS>::instance(
                ctx.clone(),
                WindowJS {
                    id: *id,
                    graphic_proxy: self.graphic_proxy.clone(),
                },
            )?;
            arr.set(i, window)?;
        }
        Ok(arr)
    }

    /// Create a new window
    ///
    /// # Arguments
    /// * `config` - Window configuration object with properties:
    ///   - title: string (default: "Staminal")
    ///   - width: number (default: 1280)
    ///   - height: number (default: 720)
    ///   - fullscreen: boolean (default: false)
    ///   - resizable: boolean (default: true)
    ///   - visible: boolean (default: true)
    ///   - positionMode: WindowPositionModes enum (default: Centered)
    ///
    /// The config uses the same format as the `window` parameter in `enableEngine()`.
    ///
    /// # Returns
    /// Promise that resolves to a Window object
    ///
    /// # Throws
    /// Error if called on server or if no engine is enabled
    #[qjs(rename = "createWindow")]
    pub async fn create_window<'js>(
        &self,
        ctx: Ctx<'js>,
        config: Opt<Object<'js>>,
    ) -> rquickjs::Result<rquickjs::Class<'js, WindowJS>> {
        tracing::debug!("GraphicJS::create_window called");

        let window_config = if let Some(cfg) = config.0 {
            let fullscreen = cfg.get::<_, bool>("fullscreen").unwrap_or(false);
            WindowConfig {
                title: cfg
                    .get::<_, String>("title")
                    .unwrap_or_else(|_| "Staminal".to_string()),
                width: cfg.get::<_, u32>("width").unwrap_or(1280),
                height: cfg.get::<_, u32>("height").unwrap_or(720),
                fullscreen,
                resizable: cfg.get::<_, bool>("resizable").unwrap_or(true),
                visible: cfg.get::<_, bool>("visible").unwrap_or(true),
                position_mode: WindowPositionMode::from_u32(
                    cfg.get::<_, u32>("positionMode").unwrap_or(1), // 1 = Centered
                ),
                mode: if fullscreen { WindowMode::Fullscreen } else { WindowMode::Windowed },
            }
        } else {
            WindowConfig::default()
        };

        let window_id = self
            .graphic_proxy
            .create_window(window_config)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))?;

        rquickjs::Class::<WindowJS>::instance(
            ctx,
            WindowJS {
                id: window_id,
                graphic_proxy: self.graphic_proxy.clone(),
            },
        )
    }

    /// Load a custom font from a file
    ///
    /// # Arguments
    /// * `alias` - Name to reference this font by (e.g., "default", "title-font")
    /// * `path` - Path to the font file, relative to the client's data directory
    ///            Use system.getAssetsPath() to resolve mod asset paths
    ///
    /// # Returns
    /// Promise that resolves to the assigned alias
    ///
    /// # Example
    /// ```javascript
    /// const fontPath = system.getAssetsPath("fonts/MyFont.ttf");
    /// await graphic.loadFont("my-font", fontPath);
    /// ```
    #[qjs(rename = "loadFont")]
    pub async fn load_font<'js>(
        &self,
        ctx: Ctx<'js>,
        alias: String,
        path: String,
    ) -> rquickjs::Result<String> {
        self.graphic_proxy
            .load_font(path, Some(alias))
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Unload a previously loaded font
    ///
    /// # Arguments
    /// * `alias` - The font alias to unload
    ///
    /// # Returns
    /// Promise that resolves when the font is unloaded
    #[qjs(rename = "unloadFont")]
    pub async fn unload_font<'js>(&self, ctx: Ctx<'js>, alias: String) -> rquickjs::Result<()> {
        self.graphic_proxy
            .unload_font(alias)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Get the primary screen/monitor identifier
    ///
    /// Returns a numeric identifier for the primary display. This identifier
    /// can be passed to `getScreenResolution()` to get the screen's resolution.
    ///
    /// # Returns
    /// Promise that resolves to a screen identifier (number)
    ///
    /// # Throws
    /// Error if called on server or if no engine is enabled
    ///
    /// # Example
    /// ```javascript
    /// const screen = await Graphic.getPrimaryScreen();
    /// const resolution = await Graphic.getScreenResolution(screen);
    /// console.log(`Primary screen: ${resolution.width}x${resolution.height}`);
    /// ```
    #[qjs(rename = "getPrimaryScreen")]
    pub async fn get_primary_screen<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<u32> {
        self.graphic_proxy
            .get_primary_screen()
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Get the resolution of a screen/monitor
    ///
    /// # Arguments
    /// * `screen_id` - Screen identifier (from `getPrimaryScreen()`)
    ///
    /// # Returns
    /// Promise that resolves to an object with `width` and `height` properties
    ///
    /// # Throws
    /// Error if called on server, no engine is enabled, or screen ID is invalid
    ///
    /// # Example
    /// ```javascript
    /// const screen = await Graphic.getPrimaryScreen();
    /// const resolution = await Graphic.getScreenResolution(screen);
    /// console.log(`Resolution: ${resolution.width}x${resolution.height}`);
    /// ```
    #[qjs(rename = "getScreenResolution")]
    pub async fn get_screen_resolution<'js>(
        &self,
        ctx: Ctx<'js>,
        screen_id: u32,
    ) -> rquickjs::Result<Object<'js>> {
        let (width, height) = self
            .graphic_proxy
            .get_screen_resolution(screen_id)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))?;

        let obj = Object::new(ctx.clone())?;
        obj.set("width", width)?;
        obj.set("height", height)?;
        Ok(obj)
    }
}

/// JavaScript Window class
///
/// Represents a window created by the graphic engine.
/// Instances are returned by `graphic.createWindow()`.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct WindowJS {
    #[qjs(skip_trace)]
    id: u64,
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
}

#[rquickjs::methods]
impl WindowJS {
    /// Get the window ID
    #[qjs(get, rename = "id")]
    pub fn get_id(&self) -> u64 {
        self.id
    }

    /// Set window size
    ///
    /// # Arguments
    /// * `width` - New width in pixels
    /// * `height` - New height in pixels
    ///
    /// # Returns
    /// Promise that resolves when size is changed
    #[qjs(rename = "setSize")]
    pub async fn set_size(&self, ctx: Ctx<'_>, width: u32, height: u32) -> rquickjs::Result<()> {
        self.graphic_proxy
            .set_window_size(self.id, width, height)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Set window title
    ///
    /// # Arguments
    /// * `title` - New window title
    ///
    /// # Returns
    /// Promise that resolves when title is changed
    #[qjs(rename = "setTitle")]
    pub async fn set_title(&self, ctx: Ctx<'_>, title: String) -> rquickjs::Result<()> {
        self.graphic_proxy
            .set_window_title(self.id, title)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Set window mode (windowed, fullscreen, borderless fullscreen)
    ///
    /// # Arguments
    /// * `mode` - WindowModes enum value (0=Windowed, 1=Fullscreen, 2=BorderlessFullscreen)
    ///
    /// # Returns
    /// Promise that resolves when mode is changed
    #[qjs(rename = "setMode")]
    pub async fn set_mode(&self, ctx: Ctx<'_>, mode: u32) -> rquickjs::Result<()> {
        let window_mode = WindowMode::from_u32(mode).ok_or_else(|| {
            let msg = format!(
                "Invalid window mode: {}. Use WindowModes.Windowed (0), Fullscreen (1), or BorderlessFullscreen (2)",
                mode
            );
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
        })?;
        self.graphic_proxy
            .set_window_mode(self.id, window_mode)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Set window visibility
    ///
    /// # Arguments
    /// * `visible` - true to show, false to hide
    ///
    /// # Returns
    /// Promise that resolves when visibility is changed
    #[qjs(rename = "setVisible")]
    pub async fn set_visible(&self, ctx: Ctx<'_>, visible: bool) -> rquickjs::Result<()> {
        self.graphic_proxy
            .set_window_visible(self.id, visible)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Close the window
    ///
    /// # Returns
    /// Promise that resolves when window is closed
    #[qjs(rename = "close")]
    pub async fn close(&self, ctx: Ctx<'_>) -> rquickjs::Result<()> {
        self.graphic_proxy
            .close_window(self.id)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    // Note: setResizable was removed - resizable must be set at window creation time via enableEngine()

    /// Create a widget in this window
    ///
    /// # Arguments
    /// * `widget_type` - WidgetTypes enum value (0=container, 1=text, 2=button, 3=image, 4=panel)
    /// * `config` - Widget configuration object
    ///
    /// # Returns
    /// Promise that resolves to a Widget object
    #[qjs(rename = "createWidget")]
    pub async fn create_widget<'js>(
        &self,
        ctx: Ctx<'js>,
        widget_type: u32,
        config: Opt<Object<'js>>,
    ) -> rquickjs::Result<rquickjs::Class<'js, WidgetJS>> {
        let wtype = WidgetType::from_u32(widget_type).ok_or_else(|| {
            let msg = format!(
                "Invalid widget type: {}. Use WidgetTypes.Container (0), Text (1), Button (2), Image (3), or Panel (4)",
                widget_type
            );
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
        })?;

        let widget_config = parse_widget_config(&ctx, config.0)?;

        let widget_id = self
            .graphic_proxy
            .create_widget(self.id, wtype, widget_config)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))?;

        rquickjs::Class::<WidgetJS>::instance(
            ctx,
            WidgetJS {
                id: widget_id,
                window_id: self.id,
                graphic_proxy: self.graphic_proxy.clone(),
            },
        )
    }

    /// Clear all widgets from this window
    ///
    /// This also removes all event handlers registered for widgets in this window.
    ///
    /// # Returns
    /// Promise that resolves when all widgets are destroyed
    #[qjs(rename = "clearWidgets")]
    pub async fn clear_widgets(&self, ctx: Ctx<'_>) -> rquickjs::Result<()> {
        // First, remove all JavaScript handlers for widgets in this window
        let widgets = self.graphic_proxy.get_window_widgets(self.id);
        for widget in &widgets {
            remove_all_widget_handlers(&ctx, widget.id)?;
        }

        // Then clear widgets from the graphic engine and proxy cache
        self.graphic_proxy
            .clear_window_widgets(self.id)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Set the default font for this window
    ///
    /// All widgets in this window will inherit this font configuration
    /// unless they override it with their own font settings.
    ///
    /// # Arguments
    /// * `family` - Font family alias (must be loaded via graphic.loadFont())
    /// * `size` - Font size in pixels
    ///
    /// # Returns
    /// Promise that resolves when the font is set
    ///
    /// # Example
    /// ```javascript
    /// await graphic.loadFont("my-font", "assets/fonts/MyFont.ttf");
    /// window.setFont("my-font", 16);
    /// ```
    #[qjs(rename = "setFont")]
    pub async fn set_font(&self, ctx: Ctx<'_>, family: String, size: f32) -> rquickjs::Result<()> {
        self.graphic_proxy
            .set_window_font(self.id, family, size)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Get the window title
    ///
    /// # Returns
    /// The current window title string
    ///
    /// # Example
    /// ```javascript
    /// const title = window.getTitle();
    /// console.log("Window title:", title);
    /// ```
    #[qjs(rename = "getTitle")]
    pub fn get_title(&self, ctx: Ctx<'_>) -> rquickjs::Result<String> {
        self.graphic_proxy
            .get_window_info(self.id)
            .map(|info| info.config.title)
            .ok_or_else(|| {
                let msg = format!("Window {} not found", self.id);
                ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
            })
    }

    /// Get the window size
    ///
    /// # Returns
    /// Object with `width` and `height` properties (in pixels)
    ///
    /// # Example
    /// ```javascript
    /// const size = window.getSize();
    /// console.log(`Window size: ${size.width}x${size.height}`);
    /// ```
    #[qjs(rename = "getSize")]
    pub fn get_size<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Object<'js>> {
        let info = self.graphic_proxy.get_window_info(self.id).ok_or_else(|| {
            let msg = format!("Window {} not found", self.id);
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
        })?;

        let size = Object::new(ctx.clone())?;
        size.set("width", info.config.width)?;
        size.set("height", info.config.height)?;
        Ok(size)
    }

    /// Get the window mode
    ///
    /// # Returns
    /// WindowModes enum value (0=Windowed, 1=Fullscreen, 2=BorderlessFullscreen)
    ///
    /// # Example
    /// ```javascript
    /// const mode = window.getMode();
    /// if (mode === WindowModes.Fullscreen) {
    ///     console.log("Window is in fullscreen mode");
    /// }
    /// ```
    #[qjs(rename = "getMode")]
    pub fn get_mode(&self, ctx: Ctx<'_>) -> rquickjs::Result<u32> {
        let info = self.graphic_proxy.get_window_info(self.id).ok_or_else(|| {
            let msg = format!("Window {} not found", self.id);
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
        })?;

        Ok(info.config.mode.to_u32())
    }
}

/// JavaScript Widget class
///
/// Represents a UI widget created in a window.
/// Instances are returned by `window.createWidget()`.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct WidgetJS {
    #[qjs(skip_trace)]
    id: u64,
    #[qjs(skip_trace)]
    window_id: u64,
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
}

#[rquickjs::methods]
impl WidgetJS {
    /// Get the widget ID
    #[qjs(get, rename = "id")]
    pub fn get_id(&self) -> u64 {
        self.id
    }

    /// Get the parent window ID
    #[qjs(get, rename = "windowId")]
    pub fn get_window_id(&self) -> u64 {
        self.window_id
    }

    /// Set text content (for Text widgets) or label (for Button widgets)
    ///
    /// # Arguments
    /// * `content` - The text content
    #[qjs(rename = "setContent")]
    pub async fn set_content(&self, ctx: Ctx<'_>, content: String) -> rquickjs::Result<()> {
        use crate::api::PropertyValue;
        self.graphic_proxy
            .update_widget_property(self.id, "content".to_string(), PropertyValue::String(content))
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Set background color
    ///
    /// # Arguments
    /// * `color` - Color string ("#RGB", "#RRGGBB", "rgba(r,g,b,a)") or color object
    #[qjs(rename = "setBackgroundColor")]
    pub async fn set_background_color<'js>(
        &self,
        ctx: Ctx<'js>,
        color: Value<'js>,
    ) -> rquickjs::Result<()> {
        use crate::api::PropertyValue;
        let color_value = parse_color(&ctx, &color)?;
        self.graphic_proxy
            .update_widget_property(
                self.id,
                "backgroundColor".to_string(),
                PropertyValue::Color(color_value),
            )
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Create a child widget
    ///
    /// # Arguments
    /// * `widget_type` - WidgetTypes enum value
    /// * `config` - Widget configuration object
    ///
    /// # Returns
    /// Promise that resolves to a Widget object
    #[qjs(rename = "createChild")]
    pub async fn create_child<'js>(
        &self,
        ctx: Ctx<'js>,
        widget_type: u32,
        config: Opt<Object<'js>>,
    ) -> rquickjs::Result<rquickjs::Class<'js, WidgetJS>> {
        let wtype = WidgetType::from_u32(widget_type).ok_or_else(|| {
            let msg = format!(
                "Invalid widget type: {}. Use WidgetTypes.Container (0), Text (1), Button (2), Image (3), or Panel (4)",
                widget_type
            );
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
        })?;

        let mut widget_config = parse_widget_config(&ctx, config.0)?;
        widget_config.parent_id = Some(self.id);

        let widget_id = self
            .graphic_proxy
            .create_widget(self.window_id, wtype, widget_config)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))?;

        rquickjs::Class::<WidgetJS>::instance(
            ctx,
            WidgetJS {
                id: widget_id,
                window_id: self.window_id,
                graphic_proxy: self.graphic_proxy.clone(),
            },
        )
    }

    /// Destroy this widget and all its children
    ///
    /// This also removes all event handlers registered for this widget and its descendants.
    #[qjs(rename = "destroy")]
    pub async fn destroy(&self, ctx: Ctx<'_>) -> rquickjs::Result<()> {
        // First, remove all JavaScript handlers for this widget and its descendants
        remove_all_widget_handlers(&ctx, self.id)?;
        let descendants = self.graphic_proxy.get_widget_descendants(self.id);
        for descendant_id in descendants {
            remove_all_widget_handlers(&ctx, descendant_id)?;
        }

        // Then destroy the widget from the graphic engine and proxy cache
        self.graphic_proxy
            .destroy_widget(self.id)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Set a widget property dynamically
    ///
    /// # Arguments
    /// * `property` - Property name (e.g., "content", "width", "backgroundColor", "disabled", "label")
    /// * `value` - Property value (string, number, boolean, or color)
    ///
    /// # Example
    /// ```javascript
    /// await widget.setProperty("content", "New text");
    /// await widget.setProperty("width", "50%");
    /// await widget.setProperty("backgroundColor", "#ff0000");
    /// await widget.setProperty("disabled", true);
    /// ```
    #[qjs(rename = "setProperty")]
    pub async fn set_property<'js>(
        &self,
        ctx: Ctx<'js>,
        property: String,
        value: Value<'js>,
    ) -> rquickjs::Result<()> {
        use crate::api::PropertyValue;

        // Parse the value based on the property name and value type
        let prop_value = parse_property_value(&ctx, &property, &value)?;

        self.graphic_proxy
            .update_widget_property(self.id, property, prop_value)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))
    }

    /// Subscribe to widget events
    ///
    /// # Arguments
    /// * `event_type` - Event type string ("click", "hover", "focus")
    /// * `callback` - JavaScript callback function
    ///
    /// # Example
    /// ```javascript
    /// await widget.on("click", () => { console.log("Clicked!"); });
    /// ```
    #[qjs(rename = "on")]
    pub async fn on<'js>(
        &self,
        ctx: Ctx<'js>,
        event_type: String,
        callback: Function<'js>,
    ) -> rquickjs::Result<()> {
        use crate::api::WidgetEventType;

        // Parse event type string to enum
        let event = match event_type.to_lowercase().as_str() {
            "click" => WidgetEventType::Click,
            "hover" => WidgetEventType::Hover,
            "focus" => WidgetEventType::Focus,
            _ => {
                return Err(throw_error(
                    &ctx,
                    &format!(
                        "Invalid event type: '{}'. Valid types are: 'click', 'hover', 'focus'",
                        event_type
                    ),
                ));
            }
        };

        // Subscribe to the event in the graphic engine
        self.graphic_proxy
            .subscribe_widget_events(self.id, vec![event])
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))?;

        // Store the callback in the widget handlers map
        store_widget_handler(&ctx, self.id, &event_type.to_lowercase(), callback)?;

        Ok(())
    }

    /// Unsubscribe from widget events
    ///
    /// # Arguments
    /// * `event_type` - Event type string ("click", "hover", "focus")
    ///
    /// # Example
    /// ```javascript
    /// await widget.off("click");
    /// ```
    #[qjs(rename = "off")]
    pub async fn off<'js>(&self, ctx: Ctx<'js>, event_type: String) -> rquickjs::Result<()> {
        use crate::api::WidgetEventType;

        // Parse event type string to enum
        let event = match event_type.to_lowercase().as_str() {
            "click" => WidgetEventType::Click,
            "hover" => WidgetEventType::Hover,
            "focus" => WidgetEventType::Focus,
            _ => {
                return Err(throw_error(
                    &ctx,
                    &format!(
                        "Invalid event type: '{}'. Valid types are: 'click', 'hover', 'focus'",
                        event_type
                    ),
                ));
            }
        };

        // Unsubscribe from the event
        self.graphic_proxy
            .unsubscribe_widget_events(self.id, vec![event])
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into()))?;

        // Remove the callback from the widget handlers map
        remove_widget_handler(&ctx, self.id, &event_type.to_lowercase())?;

        Ok(())
    }
}

// ============================================================================
// Widget Config Parsing Helpers
// ============================================================================

/// Helper to throw a JavaScript Error with stack trace
fn throw_error<'js>(ctx: &Ctx<'js>, message: &str) -> rquickjs::Error {
    // Create a proper JavaScript Error object which includes stack trace
    // We use eval to construct the Error since rquickjs Function doesn't expose a direct constructor call API
    let escaped_message = message.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    let error_code = format!("new Error(\"{}\")", escaped_message);
    match ctx.eval::<rquickjs::Value, _>(error_code) {
        Ok(error_obj) => ctx.throw(error_obj),
        Err(_) => {
            // Fallback to string throw if eval fails for some reason
            ctx.throw(rquickjs::String::from_str(ctx.clone(), message).unwrap().into())
        }
    }
}

impl WidgetType {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(WidgetType::Container),
            1 => Some(WidgetType::Text),
            2 => Some(WidgetType::Button),
            3 => Some(WidgetType::Image),
            4 => Some(WidgetType::Panel),
            _ => None,
        }
    }

    pub fn to_u32(&self) -> u32 {
        match self {
            WidgetType::Container => 0,
            WidgetType::Text => 1,
            WidgetType::Button => 2,
            WidgetType::Image => 3,
            WidgetType::Panel => 4,
        }
    }
}

/// Parse a color value from JavaScript
fn parse_color<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> rquickjs::Result<ColorValue> {
    // Handle string format: "#RGB", "#RRGGBB", "rgba(r,g,b,a)"
    if let Some(s) = value.as_string() {
        let color_str = s.to_string()?;
        return ColorValue::from_hex(&color_str).map_err(|e| {
            throw_error(ctx, &format!("Invalid color: {}", e))
        });
    }

    // Handle object format: { r, g, b, a }
    if let Some(obj) = value.as_object() {
        let r = obj.get::<_, f32>("r").unwrap_or(1.0);
        let g = obj.get::<_, f32>("g").unwrap_or(1.0);
        let b = obj.get::<_, f32>("b").unwrap_or(1.0);
        let a = obj.get::<_, f32>("a").unwrap_or(1.0);
        return Ok(ColorValue::rgba(r, g, b, a));
    }

    Err(throw_error(
        ctx,
        "Color must be a string ('#RGB', '#RRGGBB', 'rgba(r,g,b,a)') or object { r, g, b, a }",
    ))
}

/// Parse a property value from JavaScript based on property name
///
/// This function intelligently converts JavaScript values to PropertyValue
/// based on the property name and the type of value provided.
fn parse_property_value<'js>(
    ctx: &Ctx<'js>,
    property: &str,
    value: &Value<'js>,
) -> rquickjs::Result<PropertyValue> {
    // Boolean properties
    if property == "disabled" {
        if let Some(b) = value.as_bool() {
            return Ok(PropertyValue::Bool(b));
        }
        return Err(throw_error(ctx, "Property 'disabled' expects a boolean value"));
    }

    // Color properties
    let color_props = [
        "backgroundColor",
        "fontColor",
        "borderColor",
        "hoverColor",
        "pressedColor",
        "disabledColor",
    ];
    if color_props.contains(&property) {
        let color = parse_color(ctx, value)?;
        return Ok(PropertyValue::Color(color));
    }

    // Size properties
    let size_props = ["width", "height", "minWidth", "maxWidth", "minHeight", "maxHeight"];
    if size_props.contains(&property) {
        let size = parse_size_value(ctx, value)?;
        return Ok(PropertyValue::Size(size));
    }

    // Number properties
    let number_props = ["opacity", "borderRadius", "gap"];
    if number_props.contains(&property) {
        if let Some(n) = value.as_number() {
            return Ok(PropertyValue::Number(n));
        }
        return Err(throw_error(
            ctx,
            &format!("Property '{}' expects a number value", property),
        ));
    }

    // String properties (content, label, etc.)
    // Also used as fallback for unknown properties
    if let Some(s) = value.as_string() {
        return Ok(PropertyValue::String(s.to_string()?));
    }

    // If it's a number and not handled above, convert to Number
    if let Some(n) = value.as_number() {
        return Ok(PropertyValue::Number(n));
    }

    // If it's a boolean and not handled above, convert to Bool
    if let Some(b) = value.as_bool() {
        return Ok(PropertyValue::Bool(b));
    }

    Err(throw_error(
        ctx,
        &format!(
            "Cannot convert value to property '{}'. Expected string, number, or boolean.",
            property
        ),
    ))
}

/// Parse SizeValue from JavaScript value
fn parse_size_value<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> rquickjs::Result<SizeValue> {
    // Handle number (pixels)
    if let Some(n) = value.as_number() {
        return Ok(SizeValue::Px(n as f32));
    }

    // Handle string ("auto", "100%", "50px")
    if let Some(s) = value.as_string() {
        let str_val = s.to_string()?;
        if str_val == "auto" {
            return Ok(SizeValue::Auto);
        }
        if str_val.ends_with('%') {
            let pct: f32 = str_val
                .trim_end_matches('%')
                .parse()
                .map_err(|_| throw_error(ctx, "Invalid percentage"))?;
            return Ok(SizeValue::Percent(pct));
        }
        if str_val.ends_with("px") {
            let px: f32 = str_val
                .trim_end_matches("px")
                .parse()
                .map_err(|_| throw_error(ctx, "Invalid pixel value"))?;
            return Ok(SizeValue::Px(px));
        }
        // Try parsing as number
        if let Ok(n) = str_val.parse::<f32>() {
            return Ok(SizeValue::Px(n));
        }
    }

    Err(throw_error(
        ctx,
        "Size must be a number, 'auto', or string like '100%' or '50px'",
    ))
}

/// Parse EdgeInsets from JavaScript value (number, array, or object)
fn parse_edge_insets<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> rquickjs::Result<EdgeInsets> {
    // Handle number (uniform)
    if let Some(n) = value.as_number() {
        return Ok(EdgeInsets::all(n as f32));
    }

    // Handle string (e.g., "10", "10px")
    if let Some(str_val) = value.as_string() {
        let str_val = str_val.to_string().unwrap_or_default();
        let str_val = str_val.trim();

        // Parse "10px" format
        if str_val.ends_with("px") {
            if let Ok(n) = str_val.trim_end_matches("px").trim().parse::<f32>() {
                return Ok(EdgeInsets::all(n));
            }
        }
        // Parse plain number string
        if let Ok(n) = str_val.parse::<f32>() {
            return Ok(EdgeInsets::all(n));
        }
    }

    // Handle array [top, right, bottom, left] or [vertical, horizontal] or [all]
    if let Some(arr) = value.as_array() {
        let len = arr.len();
        match len {
            1 => {
                let all: f32 = arr.get(0)?;
                return Ok(EdgeInsets::all(all));
            }
            2 => {
                let vertical: f32 = arr.get(0)?;
                let horizontal: f32 = arr.get(1)?;
                return Ok(EdgeInsets::symmetric(vertical, horizontal));
            }
            4 => {
                let top: f32 = arr.get(0)?;
                let right: f32 = arr.get(1)?;
                let bottom: f32 = arr.get(2)?;
                let left: f32 = arr.get(3)?;
                return Ok(EdgeInsets {
                    top,
                    right,
                    bottom,
                    left,
                });
            }
            _ => {}
        }
    }

    // Handle object { top, right, bottom, left }
    if let Some(obj) = value.as_object() {
        return Ok(EdgeInsets {
            top: obj.get::<_, f32>("top").unwrap_or(0.0),
            right: obj.get::<_, f32>("right").unwrap_or(0.0),
            bottom: obj.get::<_, f32>("bottom").unwrap_or(0.0),
            left: obj.get::<_, f32>("left").unwrap_or(0.0),
        });
    }

    // Get type name for better error message
    let type_name = if value.is_undefined() {
        "undefined"
    } else if value.is_null() {
        "null"
    } else if value.is_bool() {
        "boolean"
    } else if value.is_function() {
        "function"
    } else {
        "unknown"
    };

    Err(throw_error(
        ctx,
        &format!(
            "Edge insets must be a number, string, array [v, h] or [t, r, b, l], or object {{ top, right, bottom, left }}. Got: {}",
            type_name
        ),
    ))
}

/// Parse widget configuration from JavaScript object
fn parse_widget_config<'js>(
    ctx: &Ctx<'js>,
    config: Option<Object<'js>>,
) -> rquickjs::Result<WidgetConfig> {
    let Some(cfg) = config else {
        return Ok(WidgetConfig::default());
    };

    let mut widget_config = WidgetConfig::default();

    // Parent ID
    if let Ok(pid) = cfg.get::<_, u64>("parentId") {
        widget_config.parent_id = Some(pid);
    }

    // Layout
    if let Ok(dir) = cfg.get::<_, u32>("direction") {
        widget_config.direction = match dir {
            0 => Some(FlexDirection::Row),
            1 => Some(FlexDirection::Column),
            2 => Some(FlexDirection::RowReverse),
            3 => Some(FlexDirection::ColumnReverse),
            _ => None,
        };
    }

    if let Ok(jc) = cfg.get::<_, u32>("justifyContent") {
        widget_config.justify_content = match jc {
            0 => Some(JustifyContent::FlexStart),
            1 => Some(JustifyContent::FlexEnd),
            2 => Some(JustifyContent::Center),
            3 => Some(JustifyContent::SpaceBetween),
            4 => Some(JustifyContent::SpaceAround),
            5 => Some(JustifyContent::SpaceEvenly),
            _ => None,
        };
    }

    if let Ok(ai) = cfg.get::<_, u32>("alignItems") {
        widget_config.align_items = match ai {
            0 => Some(AlignItems::Stretch),
            1 => Some(AlignItems::FlexStart),
            2 => Some(AlignItems::FlexEnd),
            3 => Some(AlignItems::Center),
            4 => Some(AlignItems::Baseline),
            _ => None,
        };
    }

    if let Ok(gap) = cfg.get::<_, f32>("gap") {
        widget_config.gap = Some(gap);
    }

    // Dimensions
    if let Ok(width) = cfg.get::<_, Value>("width") {
        if !width.is_undefined() && !width.is_null() {
            widget_config.width = Some(parse_size_value(ctx, &width)?);
        }
    }

    if let Ok(height) = cfg.get::<_, Value>("height") {
        if !height.is_undefined() && !height.is_null() {
            widget_config.height = Some(parse_size_value(ctx, &height)?);
        }
    }

    // Spacing
    if let Ok(margin) = cfg.get::<_, Value>("margin") {
        if !margin.is_undefined() && !margin.is_null() {
            widget_config.margin = Some(parse_edge_insets(ctx, &margin)?);
        }
    }

    if let Ok(padding) = cfg.get::<_, Value>("padding") {
        if !padding.is_undefined() && !padding.is_null() {
            widget_config.padding = Some(parse_edge_insets(ctx, &padding)?);
        }
    }

    // Colors
    if let Ok(bg_color) = cfg.get::<_, Value>("backgroundColor") {
        if !bg_color.is_undefined() && !bg_color.is_null() {
            widget_config.background_color = Some(parse_color(ctx, &bg_color)?);
        }
    }

    if let Ok(font_color) = cfg.get::<_, Value>("fontColor") {
        if !font_color.is_undefined() && !font_color.is_null() {
            widget_config.font_color = Some(parse_color(ctx, &font_color)?);
        }
    }

    if let Ok(hover_color) = cfg.get::<_, Value>("hoverColor") {
        if !hover_color.is_undefined() && !hover_color.is_null() {
            widget_config.hover_color = Some(parse_color(ctx, &hover_color)?);
        }
    }

    if let Ok(pressed_color) = cfg.get::<_, Value>("pressedColor") {
        if !pressed_color.is_undefined() && !pressed_color.is_null() {
            widget_config.pressed_color = Some(parse_color(ctx, &pressed_color)?);
        }
    }

    if let Ok(disabled_color) = cfg.get::<_, Value>("disabledColor") {
        if !disabled_color.is_undefined() && !disabled_color.is_null() {
            widget_config.disabled_color = Some(parse_color(ctx, &disabled_color)?);
        }
    }

    // Text content
    if let Ok(content) = cfg.get::<_, String>("content") {
        widget_config.content = Some(content);
    }

    if let Ok(label) = cfg.get::<_, String>("label") {
        widget_config.label = Some(label);
    }

    // Font configuration
    if let Ok(font_obj) = cfg.get::<_, Object>("font") {
        let mut font_config = FontConfig::default();
        if let Ok(family) = font_obj.get::<_, String>("family") {
            font_config.family = family;
        }
        if let Ok(size) = font_obj.get::<_, f32>("size") {
            font_config.size = size;
        }
        widget_config.font = Some(font_config);
    } else if let Ok(font_size) = cfg.get::<_, f32>("fontSize") {
        let mut font_config = FontConfig::default();
        font_config.size = font_size;
        widget_config.font = Some(font_config);
    }

    // Opacity
    if let Ok(opacity) = cfg.get::<_, f32>("opacity") {
        widget_config.opacity = Some(opacity);
    }

    // Border
    if let Ok(border_radius) = cfg.get::<_, f32>("borderRadius") {
        widget_config.border_radius = Some(border_radius);
    }

    if let Ok(border_color) = cfg.get::<_, Value>("borderColor") {
        if !border_color.is_undefined() && !border_color.is_null() {
            widget_config.border_color = Some(parse_color(ctx, &border_color)?);
        }
    }

    // Disabled state
    if let Ok(disabled) = cfg.get::<_, bool>("disabled") {
        widget_config.disabled = Some(disabled);
    }

    // Image configuration (for Image widget)
    // Can be specified as:
    // - { image: { resourceId: "alias", scaleMode: ImageScaleModes.Cover } }
    // - { resourceId: "alias", scaleMode: ImageScaleModes.Cover } (shorthand)
    let image_config = if let Ok(image_obj) = cfg.get::<_, Object>("image") {
        // Full form: { image: { ... } }
        Some(parse_image_config(ctx, &image_obj)?)
    } else if cfg.get::<_, String>("resourceId").is_ok() || cfg.get::<_, String>("path").is_ok() {
        // Shorthand: properties directly on widget config
        Some(parse_image_config(ctx, &cfg)?)
    } else {
        None
    };
    widget_config.image = image_config;

    Ok(widget_config)
}

/// Parse ImageConfig from JavaScript object
fn parse_image_config<'js>(ctx: &Ctx<'js>, obj: &Object<'js>) -> rquickjs::Result<crate::api::graphic::ImageConfig> {
    use crate::api::graphic::{ImageConfig, ImageScaleMode};

    let mut config = ImageConfig {
        path: None,
        resource_id: None,
        scale_mode: ImageScaleMode::default(),
        tint: None,
        opacity: None,
        flip_x: false,
        flip_y: false,
        source_rect: None,
    };

    // Resource ID (alias from Resource.load())
    if let Ok(resource_id) = obj.get::<_, String>("resourceId") {
        config.resource_id = Some(resource_id);
    }

    // Direct path (fallback if no resourceId)
    if let Ok(path) = obj.get::<_, String>("path") {
        config.path = Some(path);
    }

    // Scale mode (as u32 enum value)
    if let Ok(scale_mode) = obj.get::<_, u32>("scaleMode") {
        config.scale_mode = match scale_mode {
            0 => ImageScaleMode::Auto,
            1 => ImageScaleMode::Stretch,
            2 => ImageScaleMode::Tiled {
                tile_x: true,
                tile_y: true,
                stretch_value: 1.0,
            },
            3 => ImageScaleMode::Sliced {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
                center: true,
            },
            4 => ImageScaleMode::Contain,
            5 => ImageScaleMode::Cover,
            _ => ImageScaleMode::Auto,
        };
    }

    // Tint color
    if let Ok(tint) = obj.get::<_, Value>("tint") {
        if !tint.is_undefined() && !tint.is_null() {
            config.tint = Some(parse_color(ctx, &tint)?);
        }
    }

    // Opacity
    if let Ok(opacity) = obj.get::<_, f32>("opacity") {
        config.opacity = Some(opacity);
    }

    // Flip
    if let Ok(flip_x) = obj.get::<_, bool>("flipX") {
        config.flip_x = flip_x;
    }
    if let Ok(flip_y) = obj.get::<_, bool>("flipY") {
        config.flip_y = flip_y;
    }

    Ok(config)
}

/// Setup graphic API in the JavaScript context
///
/// Creates the `graphic` global object and `GraphicEngines` enum.
/// Also defines the `WindowJS` and `WidgetJS` classes for instances.
///
/// # Arguments
/// * `ctx` - The JavaScript context
/// * `graphic_proxy` - The shared GraphicProxy instance
pub fn setup_graphic_api(ctx: Ctx, graphic_proxy: Arc<GraphicProxy>) -> Result<(), rquickjs::Error> {
    // Define classes
    rquickjs::Class::<GraphicJS>::define(&ctx.globals())?;
    rquickjs::Class::<WindowJS>::define(&ctx.globals())?;
    rquickjs::Class::<WidgetJS>::define(&ctx.globals())?;

    // Create Graphic instance (capitalized for Staminal convention)
    let graphic_obj =
        rquickjs::Class::<GraphicJS>::instance(ctx.clone(), GraphicJS { graphic_proxy })?;
    ctx.globals().set("Graphic", graphic_obj)?;

    // Create GraphicEngines enum
    let engines = Object::new(ctx.clone())?;
    engines.set("Bevy", GraphicEngines::Bevy.to_u32())?;
    engines.set("Wgpu", GraphicEngines::Wgpu.to_u32())?;
    engines.set("Terminal", GraphicEngines::Terminal.to_u32())?;
    ctx.globals().set("GraphicEngines", engines)?;

    // Create WindowPositionModes enum
    let position_modes = Object::new(ctx.clone())?;
    position_modes.set("Default", WindowPositionMode::Default.to_u32())?;
    position_modes.set("Centered", WindowPositionMode::Centered.to_u32())?;
    ctx.globals().set("WindowPositionModes", position_modes)?;

    // Create WindowModes enum
    let window_modes = Object::new(ctx.clone())?;
    window_modes.set("Windowed", WindowMode::Windowed.to_u32())?;
    window_modes.set("Fullscreen", WindowMode::Fullscreen.to_u32())?;
    window_modes.set("BorderlessFullscreen", WindowMode::BorderlessFullscreen.to_u32())?;
    ctx.globals().set("WindowModes", window_modes)?;

    // Create WidgetTypes enum
    let widget_types = Object::new(ctx.clone())?;
    widget_types.set("Container", WidgetType::Container.to_u32())?;
    widget_types.set("Text", WidgetType::Text.to_u32())?;
    widget_types.set("Button", WidgetType::Button.to_u32())?;
    widget_types.set("Image", WidgetType::Image.to_u32())?;
    widget_types.set("Panel", WidgetType::Panel.to_u32())?;
    ctx.globals().set("WidgetTypes", widget_types)?;

    // Create FlexDirection enum
    let flex_dirs = Object::new(ctx.clone())?;
    flex_dirs.set("Row", 0u32)?;
    flex_dirs.set("Column", 1u32)?;
    flex_dirs.set("RowReverse", 2u32)?;
    flex_dirs.set("ColumnReverse", 3u32)?;
    ctx.globals().set("FlexDirection", flex_dirs)?;

    // Create JustifyContent enum
    let justify = Object::new(ctx.clone())?;
    justify.set("FlexStart", 0u32)?;
    justify.set("FlexEnd", 1u32)?;
    justify.set("Center", 2u32)?;
    justify.set("SpaceBetween", 3u32)?;
    justify.set("SpaceAround", 4u32)?;
    justify.set("SpaceEvenly", 5u32)?;
    ctx.globals().set("JustifyContent", justify)?;

    // Create AlignItems enum
    let align = Object::new(ctx.clone())?;
    align.set("Stretch", 0u32)?;
    align.set("FlexStart", 1u32)?;
    align.set("FlexEnd", 2u32)?;
    align.set("Center", 3u32)?;
    align.set("Baseline", 4u32)?;
    ctx.globals().set("AlignItems", align)?;

    // Create PositionType enum
    let pos_type = Object::new(ctx.clone())?;
    pos_type.set("Relative", 0u32)?;
    pos_type.set("Absolute", 1u32)?;
    ctx.globals().set("PositionType", pos_type)?;

    // Create ImageScaleModes enum
    // Maps to ImageScaleMode variants:
    // - Auto (0): Natural dimensions
    // - Stretch (1): Stretch to fill (ignores aspect ratio)
    // - Tiled (2): Repeat as pattern
    // - Sliced (3): 9-slice scaling
    // - Contain (4): Fit within bounds (may letterbox)
    // - Cover (5): Cover entire area (may crop)
    let scale_modes = Object::new(ctx.clone())?;
    scale_modes.set("Auto", 0u32)?;
    scale_modes.set("Stretch", 1u32)?;
    scale_modes.set("Tiled", 2u32)?;
    scale_modes.set("Sliced", 3u32)?;
    scale_modes.set("Contain", 4u32)?;
    scale_modes.set("Cover", 5u32)?;
    ctx.globals().set("ImageScaleModes", scale_modes)?;

    Ok(())
}

// ============================================================================
// Resource API Bindings
// ============================================================================

use crate::api::resource::{ResourceProxy, ResourceType, ResourceInfo, LoadingState};

/// JavaScript Resource API class
///
/// Exposed as the `Resource` global object in JavaScript.
/// Provides methods to load, cache, and manage resources (images, fonts, etc.).
///
/// This is a client-only API. On the server, all methods will throw an error.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct ResourceJS {
    /// The shared ResourceProxy for managing resources
    #[qjs(skip_trace)]
    resource_proxy: Arc<ResourceProxy>,
    /// The shared GraphicProxy for loading graphic resources
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
    /// The shared SystemApi for path resolution (home_dir, mod existence check)
    #[qjs(skip_trace)]
    system_api: SystemApi,
}

#[rquickjs::methods]
impl ResourceJS {
    /// Load a resource into the cache (SYNCHRONOUS)
    ///
    /// This method queues a resource for loading and returns immediately.
    /// It does NOT return a Promise - use `whenLoaded()` to wait for completion.
    ///
    /// # Arguments
    /// * `path` - The resource path (can use @mod-id/path syntax)
    /// * `alias` - Unique alias for this resource
    /// * `options` - Optional object with:
    ///   - forceReload: boolean - reload even if cached
    ///   - type: string - "image", "font", "audio", etc. (auto-detected if omitted)
    ///
    /// # Returns
    /// - `ResourceInfo` if the resource is already loaded
    /// - `undefined` if the resource was queued for loading
    ///
    /// # Example
    /// ```javascript
    /// // Queue resources for loading (synchronous, returns immediately)
    /// Resource.load("@bme-assets/images/bg.jpg", "main-bg");
    /// Resource.load("@bme-assets/images/bg2.jpg", "bg2");
    /// // getLoadingProgress() will show requested: 2
    ///
    /// // Wait for a specific resource
    /// await Resource.whenLoaded("main-bg");
    ///
    /// // Or wait for all resources
    /// while (!Resource.isLoadingCompleted()) {
    ///     await System.sleep(50);
    /// }
    /// ```
    #[qjs(rename = "load")]
    pub fn load<'js>(
        &self,
        ctx: Ctx<'js>,
        path: String,
        alias: String,
        options: Opt<Object<'js>>,
    ) -> rquickjs::Result<Value<'js>> {
        // Parse options
        let mut force_reload = false;
        let mut explicit_type: Option<ResourceType> = None;

        if let Some(opts) = options.0 {
            if let Ok(fr) = opts.get::<_, bool>("forceReload") {
                force_reload = fr;
            }
            if let Ok(type_str) = opts.get::<_, String>("type") {
                explicit_type = Some(ResourceType::from_extension(&type_str));
            }
        }

        // Determine resource type from extension if not explicitly specified
        let resource_type = explicit_type.unwrap_or_else(|| {
            // Extract extension from path
            let ext = path
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_lowercase();
            ResourceType::from_extension(&ext)
        });

        // Resolve path with @mod-id support
        let home_dir = self.system_api.get_home_dir().ok_or_else(|| {
            throw_error(&ctx, "Home directory not configured. Cannot resolve resource path.")
        })?;

        // Create path config with mod existence check
        let system_api = self.system_api.clone();
        let path_config = ModPathConfig::new(&home_dir)
            .with_mod_exists_fn(move |mod_id| {
                system_api.get_mod(mod_id).is_some()
            });

        // Resolve the path (handles @mod-id/path syntax)
        let resolved = resolve_mod_path(&path, &path_config)
            .map_err(|e| throw_error(&ctx, &e))?;

        // SYNCHRONOUS: Queue the load - this increments `requested` counter immediately
        let result = self
            .resource_proxy
            .queue_load(&resolved.relative_path, &alias, resource_type, force_reload);

        match result {
            Ok(Some(info)) => {
                // Already loaded, return existing info
                Ok(resource_info_to_js(&ctx, &info)?.into_value())
            }
            Ok(None) => {
                // Queued for loading, return undefined
                Ok(Value::new_undefined(ctx))
            }
            Err(e) => Err(throw_error(&ctx, &e)),
        }
    }

    /// Wait for a specific resource to be loaded
    ///
    /// This method waits asynchronously until the specified resource
    /// has finished loading (either successfully or with an error).
    ///
    /// # Arguments
    /// * `alias` - The alias of the resource to wait for
    ///
    /// # Returns
    /// A Promise that resolves to ResourceInfo when loaded, or rejects on error.
    ///
    /// # Example
    /// ```javascript
    /// // Queue a resource
    /// Resource.load("@bme-assets/images/bg.jpg", "main-bg");
    ///
    /// // Wait for it to load
    /// const info = await Resource.whenLoaded("main-bg");
    /// console.log(`Loaded: ${info.resolvedPath}`);
    /// ```
    #[qjs(rename = "whenLoaded")]
    pub async fn when_loaded<'js>(
        &self,
        ctx: Ctx<'js>,
        alias: String,
    ) -> rquickjs::Result<Object<'js>> {
        let result = self.resource_proxy.when_loaded(&alias).await;

        match result {
            Ok(info) => resource_info_to_js(&ctx, &info),
            Err(e) => Err(throw_error(&ctx, &e)),
        }
    }

    /// Wait for all requested resources to be loaded
    ///
    /// This method waits asynchronously until all resources queued via `load()`
    /// have finished loading (either successfully or with errors).
    ///
    /// # Returns
    /// A Promise that resolves when all resources are loaded.
    /// If any resources failed, the Promise rejects with an array of error objects.
    ///
    /// # Example
    /// ```javascript
    /// // Queue multiple resources
    /// Resource.load("@assets/bg1.png", "bg1");
    /// Resource.load("@assets/bg2.png", "bg2");
    /// Resource.load("@assets/font.ttf", "font");
    ///
    /// // Wait for all to complete
    /// try {
    ///     await Resource.whenLoadedAll();
    ///     console.log("All resources loaded!");
    /// } catch (errors) {
    ///     for (const err of errors) {
    ///         console.error(`Failed: ${err.alias} - ${err.error}`);
    ///     }
    /// }
    /// ```
    #[qjs(rename = "whenLoadedAll")]
    pub async fn when_loaded_all<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<()> {
        let result = self.resource_proxy.when_loaded_all().await;

        match result {
            Ok(()) => Ok(()),
            Err(errors) => {
                // Convert errors to JavaScript array of objects
                let array = Array::new(ctx.clone())?;
                for (i, (alias, error)) in errors.iter().enumerate() {
                    let obj = Object::new(ctx.clone())?;
                    obj.set("alias", alias.clone())?;
                    obj.set("error", error.clone())?;
                    array.set(i, obj)?;
                }
                Err(throw_error(&ctx, &format!("{} resource(s) failed to load", errors.len())))
            }
        }
    }

    /// Unload a resource from the cache
    ///
    /// # Arguments
    /// * `alias` - The alias of the resource to unload
    ///
    /// # Example
    /// ```javascript
    /// await Resource.unload("main-bg");
    /// ```
    #[qjs(rename = "unload")]
    pub async fn unload<'js>(&self, ctx: Ctx<'js>, alias: String) -> rquickjs::Result<()> {
        self.resource_proxy
            .unload_resource(&alias, &self.graphic_proxy)
            .await
            .map_err(|e| throw_error(&ctx, &e))
    }

    /// Unload all resources from the cache
    ///
    /// # Example
    /// ```javascript
    /// await Resource.unloadAll();
    /// ```
    #[qjs(rename = "unloadAll")]
    pub async fn unload_all<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<()> {
        self.resource_proxy
            .unload_all_resources(&self.graphic_proxy)
            .await
            .map_err(|e| throw_error(&ctx, &e))
    }

    /// Get information about a loaded resource
    ///
    /// # Arguments
    /// * `alias` - The alias of the resource
    ///
    /// # Returns
    /// Resource info object or null if not found
    ///
    /// # Example
    /// ```javascript
    /// const info = Resource.getInfo("main-bg");
    /// if (info) {
    ///     console.log(`Resource state: ${info.state}`);
    /// }
    /// ```
    #[qjs(rename = "getInfo")]
    pub fn get_info<'js>(&self, ctx: Ctx<'js>, alias: String) -> rquickjs::Result<Value<'js>> {
        match self.resource_proxy.get_resource_info(&alias) {
            Some(info) => Ok(resource_info_to_js(&ctx, &info)?.into_value()),
            None => Ok(Value::new_null(ctx)),
        }
    }

    /// Check if a resource is loaded
    ///
    /// # Arguments
    /// * `alias` - The alias to check
    ///
    /// # Returns
    /// true if the resource exists in the cache
    ///
    /// # Example
    /// ```javascript
    /// if (Resource.isLoaded("main-bg")) {
    ///     // Use the resource
    /// }
    /// ```
    #[qjs(rename = "isLoaded")]
    pub fn is_loaded(&self, alias: String) -> bool {
        self.resource_proxy.is_resource_loaded(&alias)
    }

    /// Get current loading progress
    ///
    /// # Returns
    /// Object with { requested: number, loaded: number }
    ///
    /// # Example
    /// ```javascript
    /// const progress = Resource.getLoadingProgress();
    /// console.log(`Loaded ${progress.loaded} of ${progress.requested} resources`);
    /// ```
    #[qjs(rename = "getLoadingProgress")]
    pub fn get_loading_progress<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Object<'js>> {
        let state = self.resource_proxy.get_loading_state();
        loading_state_to_js(&ctx, &state)
    }

    /// Check if all requested resources have finished loading
    ///
    /// # Returns
    /// true if all resources are loaded (requested == loaded)
    ///
    /// # Example
    /// ```javascript
    /// // Wait for all resources to load
    /// while (!Resource.isLoadingCompleted()) {
    ///     await sleep(100);
    /// }
    /// console.log("All resources loaded!");
    /// ```
    #[qjs(rename = "isLoadingCompleted")]
    pub fn is_loading_completed(&self) -> bool {
        self.resource_proxy.is_loading_completed()
    }

    /// List all loaded resource aliases
    ///
    /// # Returns
    /// Array of alias strings
    ///
    /// # Example
    /// ```javascript
    /// const aliases = Resource.listLoaded();
    /// console.log(`Loaded resources: ${aliases.join(", ")}`);
    /// ```
    #[qjs(rename = "listLoaded")]
    pub fn list_loaded<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Array<'js>> {
        let aliases = self.resource_proxy.list_loaded_resources();
        let array = Array::new(ctx.clone())?;
        for (i, alias) in aliases.iter().enumerate() {
            array.set(i, alias.clone())?;
        }
        Ok(array)
    }
}

/// Convert ResourceInfo to JavaScript object
fn resource_info_to_js<'js>(ctx: &Ctx<'js>, info: &ResourceInfo) -> rquickjs::Result<Object<'js>> {
    let obj = Object::new(ctx.clone())?;
    obj.set("alias", info.alias.clone())?;
    obj.set("path", info.path.clone())?;
    obj.set("resolvedPath", info.resolved_path.clone())?;
    obj.set("type", info.resource_type.as_str())?;
    obj.set("state", info.state.as_str())?;

    if let Some(size) = info.size {
        obj.set("size", size as f64)?;
    } else {
        obj.set("size", rquickjs::Null)?;
    }

    if let Some(ref error) = info.error {
        obj.set("error", error.clone())?;
    } else {
        obj.set("error", rquickjs::Null)?;
    }

    Ok(obj)
}

/// Convert LoadingState to JavaScript object
fn loading_state_to_js<'js>(ctx: &Ctx<'js>, state: &LoadingState) -> rquickjs::Result<Object<'js>> {
    let obj = Object::new(ctx.clone())?;
    obj.set("requested", state.requested)?;
    obj.set("loaded", state.loaded)?;
    Ok(obj)
}

/// Setup Resource API in the JavaScript context
///
/// Creates the `Resource` global object and `ResourceTypes` enum.
///
/// # Arguments
/// * `ctx` - The JavaScript context
/// * `resource_proxy` - The shared ResourceProxy instance
/// * `graphic_proxy` - The shared GraphicProxy instance (for loading graphic resources)
/// * `system_api` - The shared SystemApi for path resolution
pub fn setup_resource_api(
    ctx: Ctx,
    resource_proxy: Arc<ResourceProxy>,
    graphic_proxy: Arc<GraphicProxy>,
    system_api: SystemApi,
) -> Result<(), rquickjs::Error> {
    // Define the class
    rquickjs::Class::<ResourceJS>::define(&ctx.globals())?;

    // Create Resource instance
    let resource_obj = rquickjs::Class::<ResourceJS>::instance(
        ctx.clone(),
        ResourceJS {
            resource_proxy,
            graphic_proxy,
            system_api,
        },
    )?;
    ctx.globals().set("Resource", resource_obj)?;

    // Create ResourceTypes enum
    let resource_types = Object::new(ctx.clone())?;
    resource_types.set("Image", ResourceType::Image.as_str())?;
    resource_types.set("Audio", ResourceType::Audio.as_str())?;
    resource_types.set("Video", ResourceType::Video.as_str())?;
    resource_types.set("Shader", ResourceType::Shader.as_str())?;
    resource_types.set("Font", ResourceType::Font.as_str())?;
    resource_types.set("Model3D", ResourceType::Model3D.as_str())?;
    resource_types.set("Json", ResourceType::Json.as_str())?;
    resource_types.set("Text", ResourceType::Text.as_str())?;
    resource_types.set("Binary", ResourceType::Binary.as_str())?;
    ctx.globals().set("ResourceTypes", resource_types)?;

    Ok(())
}

// ============================================================================
// ECS World API Bindings
// ============================================================================

use crate::api::graphic::ecs::{
    ComponentSchema, DeclaredSystem, FieldType, QueryOptions, QueryResult, SystemBehavior,
};

/// JavaScript World API class
///
/// Exposed as the `World` global object in JavaScript.
/// Provides ECS (Entity-Component-System) functionality for creating entities,
/// managing components, querying entities, and declaring systems.
///
/// This is a client-only API. On the server, all methods will throw an error.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct WorldJS {
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
}

/// JavaScript Entity handle class
///
/// Returned by World.spawn(), provides methods to manipulate a single entity.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct EntityJS {
    /// Entity ID
    id: u64,
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
}

#[rquickjs::methods]
impl WorldJS {
    /// Spawn a new entity with optional initial components
    ///
    /// # Arguments
    /// * `components` - Optional object with component_name: component_data pairs
    /// * `parent` - Optional parent entity ID or Entity handle. If provided, the new entity
    ///              will be a child of this entity. For UI entities, this determines layout hierarchy.
    ///
    /// # Returns
    /// Promise that resolves to an Entity handle
    ///
    /// # Throws
    /// Error if called on server or if graphic engine is not enabled
    ///
    /// # Example
    /// ```javascript
    /// // Spawn with no parent (will be parented to window root if UI entity)
    /// const entity = await World.spawn({
    ///     Position: { x: 0, y: 0 },
    ///     Velocity: { x: 1, y: 0 }
    /// });
    ///
    /// // Spawn as child of another entity
    /// const child = await World.spawn({
    ///     Node: { width: "100px", height: "50px" }
    /// }, parent.id);
    /// ```
    #[qjs(rename = "spawn")]
    pub async fn spawn<'js>(
        &self,
        ctx: Ctx<'js>,
        components: Opt<Object<'js>>,
        parent: Opt<u64>,
    ) -> rquickjs::Result<rquickjs::Class<'js, EntityJS>> {
        // Get mod ID from global __MOD_ID__ variable for ownership tracking
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());

        // Collect event callbacks (on_click, on_hover, etc.) before serializing
        // These are stored in __ENTITY_EVENT_CALLBACKS__[entityId][eventType] = callback
        let mut event_callbacks: Vec<(String, Function<'js>)> = Vec::new();

        // Convert JavaScript object to HashMap
        let mut component_map = std::collections::HashMap::new();
        if let Some(ref obj) = components.0 {
            for result in obj.props::<String, Value>() {
                if let Ok((key, value)) = result {
                    // Check if this is a component with event callbacks (e.g., Button with on_click)
                    if let Some(comp_obj) = value.as_object() {
                        // Clone the component object to remove callbacks before serializing
                        let filtered_obj = Object::new(ctx.clone())?;
                        for prop_result in comp_obj.props::<String, Value>() {
                            if let Ok((prop_key, prop_value)) = prop_result {
                                // Check if this is an event callback (starts with "on_")
                                if prop_key.starts_with("on_") {
                                    // Extract the event type (e.g., "on_click" -> "click")
                                    let event_type = prop_key.strip_prefix("on_").unwrap().to_string();
                                    // Check if the value is a function
                                    if let Some(func) = prop_value.as_function() {
                                        event_callbacks.push((event_type, func.clone()));
                                    }
                                    // Don't add this property to the filtered object (not serializable)
                                } else {
                                    // Regular property, keep it
                                    filtered_obj.set(&prop_key, prop_value)?;
                                }
                            }
                        }
                        // Serialize the filtered object (without callbacks)
                        if let Ok(json_str) = ctx.json_stringify(filtered_obj.clone().into_value()) {
                            if let Some(s) = json_str {
                                if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&s.to_string()?) {
                                    component_map.insert(key, json_value);
                                }
                            }
                        }
                    } else {
                        // Not an object, serialize directly
                        if let Ok(json_str) = ctx.json_stringify(value.clone()) {
                            if let Some(s) = json_str {
                                if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&s.to_string()?) {
                                    component_map.insert(key, json_value);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Spawn the entity with optional parent
        let entity_id = self
            .graphic_proxy
            .spawn_entity(component_map, mod_id, parent.0)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })?;

        // Register event callbacks if any were found
        if !event_callbacks.is_empty() {
            // Get or create the global callback registry
            let globals = ctx.globals();
            let registry: Object = match globals.get("__ENTITY_EVENT_CALLBACKS__") {
                Ok(r) => r,
                Err(_) => {
                    let new_registry = Object::new(ctx.clone())?;
                    globals.set("__ENTITY_EVENT_CALLBACKS__", new_registry.clone())?;
                    new_registry
                }
            };

            // Create entity's callback map
            let entity_callbacks = Object::new(ctx.clone())?;

            for (event_type, callback) in event_callbacks {
                // Store callback in JS registry
                entity_callbacks.set(&event_type, callback)?;

                // Register with graphic engine for direct dispatch
                self.graphic_proxy
                    .register_entity_event_callback(entity_id, &event_type)
                    .await
                    .map_err(|e| {
                        ctx.throw(
                            rquickjs::String::from_str(ctx.clone(), &e)
                                .unwrap()
                                .into(),
                        )
                    })?;
            }

            // Store under entity ID
            registry.set(entity_id.to_string(), entity_callbacks)?;
        }

        // Create and return an Entity handle
        rquickjs::Class::<EntityJS>::instance(
            ctx,
            EntityJS {
                id: entity_id,
                graphic_proxy: self.graphic_proxy.clone(),
            },
        )
    }

    /// Despawn an entity by ID
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID to despawn (can be number or Entity handle)
    ///
    /// # Returns
    /// Promise that resolves when entity is despawned
    #[qjs(rename = "despawn")]
    pub async fn despawn<'js>(&self, ctx: Ctx<'js>, entity_id: u64) -> rquickjs::Result<()> {
        self.graphic_proxy
            .despawn_entity(entity_id)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Query entities matching criteria
    ///
    /// # Arguments
    /// * `options` - Query options object:
    ///   - withComponents: array of component names that entities must have
    ///   - withoutComponents: array of component names that entities must NOT have
    ///   - limit: optional maximum number of results
    ///
    /// # Returns
    /// Promise that resolves to array of query results
    ///
    /// # Example
    /// ```javascript
    /// const entities = await World.query({
    ///     withComponents: ["Position", "Velocity"],
    ///     withoutComponents: ["Frozen"],
    ///     limit: 100
    /// });
    /// for (const entity of entities) {
    ///     console.log(entity.id, entity.components.Position);
    /// }
    /// ```
    #[qjs(rename = "query")]
    pub async fn query<'js>(
        &self,
        ctx: Ctx<'js>,
        options: Object<'js>,
    ) -> rquickjs::Result<Array<'js>> {
        // Parse query options
        let with_components: Vec<String> = options
            .get::<_, Array>("withComponents")
            .ok()
            .map(|arr| {
                arr.iter::<String>()
                    .filter_map(|r| r.ok())
                    .collect()
            })
            .unwrap_or_default();

        let without_components: Vec<String> = options
            .get::<_, Array>("withoutComponents")
            .ok()
            .map(|arr| {
                arr.iter::<String>()
                    .filter_map(|r| r.ok())
                    .collect()
            })
            .unwrap_or_default();

        let limit: Option<usize> = options.get("limit").ok();

        let query_options = QueryOptions {
            with_components,
            without_components,
            limit,
        };

        // Execute query
        let results = self
            .graphic_proxy
            .query_entities(query_options)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })?;

        // Convert results to JavaScript array
        let result_array = Array::new(ctx.clone())?;
        for (i, result) in results.iter().enumerate() {
            let obj = Object::new(ctx.clone())?;
            obj.set("id", result.entity_id)?;

            // Convert components to JS object
            let components_obj = Object::new(ctx.clone())?;
            for (name, data) in &result.components {
                let json_str = serde_json::to_string(data).unwrap_or_default();
                if let Ok(parsed) = ctx.json_parse(json_str) {
                    components_obj.set(name.as_str(), parsed)?;
                }
            }
            obj.set("components", components_obj)?;

            result_array.set(i, obj)?;
        }

        Ok(result_array)
    }

    /// Register a custom component type with optional schema
    ///
    /// # Arguments
    /// * `name` - Component type name
    /// * `schema` - Optional schema object for validation
    ///
    /// # Example
    /// ```javascript
    /// await World.registerComponent("Player", {
    ///     health: "number",
    ///     name: "string",
    ///     position: "vec2"
    /// });
    /// ```
    #[qjs(rename = "registerComponent")]
    pub async fn register_component<'js>(
        &self,
        ctx: Ctx<'js>,
        name: String,
        schema: Opt<Object<'js>>,
    ) -> rquickjs::Result<()> {
        let mut component_schema = ComponentSchema::new(name);

        // Parse schema fields if provided
        if let Some(schema_obj) = schema.0 {
            for result in schema_obj.props::<String, Value>() {
                if let Ok((field_name, type_value)) = result {
                    if let Some(type_str) = type_value.as_string() {
                        let type_string = type_str.to_string()?;
                        let field_type = match type_string.as_str() {
                            "number" => FieldType::Number,
                            "string" => FieldType::String,
                            "bool" | "boolean" => FieldType::Bool,
                            "vec2" => FieldType::Vec2,
                            "vec3" => FieldType::Vec3,
                            "color" => FieldType::Color,
                            "entity" => FieldType::Entity,
                            "any" => FieldType::Any,
                            _ => FieldType::Any,
                        };
                        component_schema.fields.insert(field_name, field_type);
                    }
                }
            }
        }

        self.graphic_proxy
            .register_component(component_schema)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Declare a system with a predefined behavior or formulas
    ///
    /// # Arguments
    /// * `config` - System configuration object:
    ///   - name: unique system name
    ///   - query: { withComponents: [...], withoutComponents: [...] }
    ///   - behavior: SystemBehaviors enum value (optional)
    ///   - config: behavior configuration (optional)
    ///   - formulas: array of formula strings (optional)
    ///   - order: execution order (default 0)
    ///   - enabled: whether system is active (default true)
    ///
    /// # Example
    /// ```javascript
    /// await World.declareSystem({
    ///     name: "gravity",
    ///     query: { withComponents: ["Velocity"] },
    ///     behavior: SystemBehaviors.ApplyGravity,
    ///     config: { strength: 9.8 },
    ///     order: 0,
    ///     enabled: true
    /// });
    /// ```
    #[qjs(rename = "declareSystem")]
    pub async fn declare_system<'js>(
        &self,
        ctx: Ctx<'js>,
        config: Object<'js>,
    ) -> rquickjs::Result<()> {
        // Parse system configuration
        let name: String = config.get("name")?;

        // Parse query options
        let query_obj: Object = config.get("query")?;
        let with_components: Vec<String> = query_obj
            .get::<_, Array>("withComponents")
            .ok()
            .map(|arr| {
                arr.iter::<String>()
                    .filter_map(|r| r.ok())
                    .collect()
            })
            .unwrap_or_default();
        let without_components: Vec<String> = query_obj
            .get::<_, Array>("withoutComponents")
            .ok()
            .map(|arr| {
                arr.iter::<String>()
                    .filter_map(|r| r.ok())
                    .collect()
            })
            .unwrap_or_default();

        let query = QueryOptions {
            with_components,
            without_components,
            limit: None,
        };

        // Parse behavior (optional)
        let behavior: Option<SystemBehavior> = config
            .get::<_, u32>("behavior")
            .ok()
            .map(|b| match b {
                0 => SystemBehavior::ApplyVelocity,
                1 => SystemBehavior::ApplyGravity,
                2 => SystemBehavior::ApplyFriction,
                3 => SystemBehavior::RegenerateOverTime,
                4 => SystemBehavior::DecayOverTime,
                5 => SystemBehavior::FollowEntity,
                6 => SystemBehavior::OrbitAround,
                7 => SystemBehavior::BounceOnBounds,
                8 => SystemBehavior::DespawnWhenZero,
                9 => SystemBehavior::AnimateSprite,
                _ => SystemBehavior::ApplyVelocity,
            });

        // Parse behavior config (optional)
        let behavior_config: Option<serde_json::Value> = config
            .get::<_, Value>("config")
            .ok()
            .and_then(|v| {
                ctx.json_stringify(v)
                    .ok()
                    .flatten()
                    .and_then(|s| s.to_string().ok())
                    .and_then(|s| serde_json::from_str(&s).ok())
            });

        // Parse formulas (optional)
        let formulas: Option<Vec<String>> = config
            .get::<_, Array>("formulas")
            .ok()
            .map(|arr| {
                arr.iter::<String>()
                    .filter_map(|r| r.ok())
                    .collect()
            });

        let order: i32 = config.get("order").unwrap_or(0);
        let enabled: bool = config.get("enabled").unwrap_or(true);

        let system = DeclaredSystem {
            name,
            query,
            behavior,
            config: behavior_config,
            formulas,
            enabled,
            order,
        };

        self.graphic_proxy
            .declare_system(system)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Enable or disable a declared system
    #[qjs(rename = "setSystemEnabled")]
    pub async fn set_system_enabled<'js>(
        &self,
        ctx: Ctx<'js>,
        name: String,
        enabled: bool,
    ) -> rquickjs::Result<()> {
        self.graphic_proxy
            .set_system_enabled(name, enabled)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Remove a declared system
    #[qjs(rename = "removeSystem")]
    pub async fn remove_system<'js>(&self, ctx: Ctx<'js>, name: String) -> rquickjs::Result<()> {
        self.graphic_proxy
            .remove_system(name)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Register an event callback for an entity
    ///
    /// When registered, the engine will send direct callback events for this
    /// entity instead of generic interaction events.
    /// This enables efficient direct callback dispatch.
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID to register a callback for
    /// * `event_type` - The event type (e.g., "click", "hover", "enter", "leave")
    ///
    /// # Example
    /// ```javascript
    /// // After spawning, register for direct click callbacks
    /// await World.registerEntityEventCallback(entity.id, "click");
    /// ```
    #[qjs(rename = "registerEntityEventCallback")]
    pub async fn register_entity_event_callback<'js>(
        &self,
        ctx: Ctx<'js>,
        entity_id: u64,
        event_type: String,
    ) -> rquickjs::Result<()> {
        self.graphic_proxy
            .register_entity_event_callback(entity_id, &event_type)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Unregister an event callback for an entity
    ///
    /// After unregistering, the engine will revert to sending generic
    /// interaction events for this entity.
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID to unregister
    /// * `event_type` - The event type to unregister
    #[qjs(rename = "unregisterEntityEventCallback")]
    pub async fn unregister_entity_event_callback<'js>(
        &self,
        ctx: Ctx<'js>,
        entity_id: u64,
        event_type: String,
    ) -> rquickjs::Result<()> {
        self.graphic_proxy
            .unregister_entity_event_callback(entity_id, &event_type)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }
}

#[rquickjs::methods]
impl EntityJS {
    /// Get the entity ID
    #[qjs(get, rename = "id")]
    pub fn get_id(&self) -> u64 {
        self.id
    }

    /// Insert or update component(s) on this entity
    ///
    /// Supports two call signatures:
    /// - `entity.insert("ComponentName", data)` - Insert a single component
    /// - `entity.insert({ ComponentName: data, OtherComponent: data2 })` - Batch insert multiple components
    ///
    /// # Arguments
    /// * `name_or_components` - Either a component name (string) or an object with component names as keys
    /// * `data` - Component data (only when first argument is a string)
    ///
    /// # Example
    /// ```javascript
    /// // Single component
    /// await entity.insert("Health", { current: 100, max: 100 });
    ///
    /// // Batch insert
    /// await entity.insert({
    ///     Node: { width: 200, height: 50 },
    ///     BackgroundColor: "#ff0000"
    /// });
    /// ```
    #[qjs(rename = "insert")]
    pub async fn insert<'js>(
        &self,
        ctx: Ctx<'js>,
        name_or_components: Value<'js>,
        data: Opt<Value<'js>>,
    ) -> rquickjs::Result<()> {
        // Check if first argument is a string (single component) or object (batch)
        if let Some(component_name) = name_or_components.as_string() {
            // Single component mode: insert("ComponentName", data)
            let component_name = component_name.to_string()?;
            let data_value = data.0.ok_or_else(|| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), "insert() with component name requires a second argument for data")
                        .unwrap()
                        .into(),
                )
            })?;

            let json_value = ctx
                .json_stringify(data_value)
                .ok()
                .flatten()
                .and_then(|s| s.to_string().ok())
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(serde_json::Value::Null);

            self.graphic_proxy
                .insert_component(self.id, component_name, json_value)
                .await
                .map_err(|e| {
                    ctx.throw(
                        rquickjs::String::from_str(ctx.clone(), &e)
                            .unwrap()
                            .into(),
                    )
                })
        } else if let Some(obj) = name_or_components.as_object() {
            // Batch mode: insert({ ComponentName: data, ... })
            for result in obj.props::<String, Value>() {
                if let Ok((component_name, component_data)) = result {
                    let json_value = ctx
                        .json_stringify(component_data)
                        .ok()
                        .flatten()
                        .and_then(|s| s.to_string().ok())
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or(serde_json::Value::Null);

                    self.graphic_proxy
                        .insert_component(self.id, component_name, json_value)
                        .await
                        .map_err(|e| {
                            ctx.throw(
                                rquickjs::String::from_str(ctx.clone(), &e)
                                    .unwrap()
                                    .into(),
                            )
                        })?;
                }
            }
            Ok(())
        } else {
            Err(ctx.throw(
                rquickjs::String::from_str(ctx.clone(), "insert() expects either a string (component name) or an object (batch components)")
                    .unwrap()
                    .into(),
            ))
        }
    }

    /// Update specific fields of component(s) on this entity (merge with existing)
    ///
    /// Unlike `insert` which replaces the entire component, `update` merges
    /// the provided fields with the existing component data.
    ///
    /// Supports two call signatures:
    /// - `entity.update("ComponentName", data)` - Update a single component
    /// - `entity.update({ ComponentName: data, OtherComponent: data2 })` - Batch update multiple components
    ///
    /// # Arguments
    /// * `name_or_components` - Either a component name (string) or an object with component names as keys
    /// * `data` - Partial component data to merge (only when first argument is a string)
    ///
    /// # Example
    /// ```javascript
    /// // Single component - only update width, keep other Node properties
    /// await entity.update("Node", { width: "50%" });
    ///
    /// // Batch update
    /// await entity.update({
    ///     Node: { width: "50%" },
    ///     Text: { value: "New text" }
    /// });
    /// ```
    #[qjs(rename = "update")]
    pub async fn update<'js>(
        &self,
        ctx: Ctx<'js>,
        name_or_components: Value<'js>,
        data: Opt<Value<'js>>,
    ) -> rquickjs::Result<()> {
        // Check if first argument is a string (single component) or object (batch)
        if let Some(component_name) = name_or_components.as_string() {
            // Single component mode: update("ComponentName", data)
            let component_name = component_name.to_string()?;
            let data_value = data.0.ok_or_else(|| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), "update() with component name requires a second argument for data")
                        .unwrap()
                        .into(),
                )
            })?;

            let json_value = ctx
                .json_stringify(data_value)
                .ok()
                .flatten()
                .and_then(|s| s.to_string().ok())
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(serde_json::Value::Null);

            self.graphic_proxy
                .update_component(self.id, component_name, json_value)
                .await
                .map_err(|e| {
                    ctx.throw(
                        rquickjs::String::from_str(ctx.clone(), &e)
                            .unwrap()
                            .into(),
                    )
                })
        } else if let Some(obj) = name_or_components.as_object() {
            // Batch mode: update({ ComponentName: data, ... })
            for result in obj.props::<String, Value>() {
                if let Ok((component_name, component_data)) = result {
                    let json_value = ctx
                        .json_stringify(component_data)
                        .ok()
                        .flatten()
                        .and_then(|s| s.to_string().ok())
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or(serde_json::Value::Null);

                    self.graphic_proxy
                        .update_component(self.id, component_name, json_value)
                        .await
                        .map_err(|e| {
                            ctx.throw(
                                rquickjs::String::from_str(ctx.clone(), &e)
                                    .unwrap()
                                    .into(),
                            )
                        })?;
                }
            }
            Ok(())
        } else {
            Err(ctx.throw(
                rquickjs::String::from_str(ctx.clone(), "update() expects either a string (component name) or an object (batch components)")
                    .unwrap()
                    .into(),
            ))
        }
    }

    /// Remove a component from this entity
    #[qjs(rename = "remove")]
    pub async fn remove<'js>(&self, ctx: Ctx<'js>, component_name: String) -> rquickjs::Result<()> {
        self.graphic_proxy
            .remove_component(self.id, component_name)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Get a component's data from this entity
    ///
    /// # Returns
    /// The component data, or null if not present
    #[qjs(rename = "get")]
    pub async fn get<'js>(
        &self,
        ctx: Ctx<'js>,
        component_name: String,
    ) -> rquickjs::Result<Value<'js>> {
        let result = self
            .graphic_proxy
            .get_component(self.id, component_name)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })?;

        match result {
            Some(data) => {
                let json_str = serde_json::to_string(&data).unwrap_or_default();
                ctx.json_parse(json_str)
            }
            None => Ok(Value::new_null(ctx)),
        }
    }

    /// Check if this entity has a component
    #[qjs(rename = "has")]
    pub async fn has<'js>(&self, ctx: Ctx<'js>, component_name: String) -> rquickjs::Result<bool> {
        self.graphic_proxy
            .has_component(self.id, component_name)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }

    /// Despawn this entity
    #[qjs(rename = "despawn")]
    pub async fn despawn<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<()> {
        self.graphic_proxy
            .despawn_entity(self.id)
            .await
            .map_err(|e| {
                ctx.throw(
                    rquickjs::String::from_str(ctx.clone(), &e)
                        .unwrap()
                        .into(),
                )
            })
    }
}

/// Set up the World API in a JavaScript context
///
/// # Arguments
/// * `ctx` - JavaScript context
/// * `graphic_proxy` - The shared GraphicProxy instance
pub fn setup_world_api(
    ctx: Ctx,
    graphic_proxy: Arc<GraphicProxy>,
) -> Result<(), rquickjs::Error> {
    // Define classes
    rquickjs::Class::<WorldJS>::define(&ctx.globals())?;
    rquickjs::Class::<EntityJS>::define(&ctx.globals())?;

    // Create World instance
    let world_obj = rquickjs::Class::<WorldJS>::instance(
        ctx.clone(),
        WorldJS {
            graphic_proxy,
        },
    )?;
    ctx.globals().set("World", world_obj)?;

    // Create SystemBehaviors enum
    let behaviors = Object::new(ctx.clone())?;
    behaviors.set("ApplyVelocity", 0u32)?;
    behaviors.set("ApplyGravity", 1u32)?;
    behaviors.set("ApplyFriction", 2u32)?;
    behaviors.set("RegenerateOverTime", 3u32)?;
    behaviors.set("DecayOverTime", 4u32)?;
    behaviors.set("FollowEntity", 5u32)?;
    behaviors.set("OrbitAround", 6u32)?;
    behaviors.set("BounceOnBounds", 7u32)?;
    behaviors.set("DespawnWhenZero", 8u32)?;
    behaviors.set("AnimateSprite", 9u32)?;
    ctx.globals().set("SystemBehaviors", behaviors)?;

    // Create FieldTypes enum for component schema definitions
    let field_types = Object::new(ctx.clone())?;
    field_types.set("Number", "number")?;
    field_types.set("String", "string")?;
    field_types.set("Bool", "bool")?;
    field_types.set("Vec2", "vec2")?;
    field_types.set("Vec3", "vec3")?;
    field_types.set("Color", "color")?;
    field_types.set("Entity", "entity")?;
    field_types.set("Any", "any")?;
    ctx.globals().set("FieldTypes", field_types)?;

    Ok(())
}
