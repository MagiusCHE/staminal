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

use crate::api::{AppApi, ConsoleApi, LocaleApi, NetworkApi, RequestUriProtocol, SystemApi, SystemEvents, ModSide};
use rquickjs::{Array, Ctx, Function, JsLifetime, Object, class::Trace};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
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

    // Create wrapper console object in JavaScript that handles any value types
    // The wrapper converts arguments to strings using JSON.stringify for objects
    ctx.eval::<(), _>(
        r#"
        const __formatArg = (arg) => {
            if (arg === undefined) return 'undefined';
            if (arg === null) return 'null';
            if (typeof arg === 'string') return arg;
            if (typeof arg === 'number' || typeof arg === 'boolean') return String(arg);
            if (typeof arg === 'function') return '[Function]';
            try {
                return JSON.stringify(arg,null,2);
            } catch (e) {
                return '[object]';
            }
        };
        const __formatArgs = (...args) => args.map(__formatArg).join(' ');

        globalThis.console = {
            log: (...args) => __console_native._log(__formatArgs(...args)),
            error: (...args) => __console_native._error(__formatArgs(...args)),
            warn: (...args) => __console_native._warn(__formatArgs(...args)),
            info: (...args) => __console_native._info(__formatArgs(...args)),
            debug: (...args) => __console_native._debug(__formatArgs(...args)),
        };

        // Setup global error handlers for uncaught errors and unhandled promise rejections
        globalThis.onerror = (message, source, lineno, colno, error) => {
            const errorMsg = error ? (error.stack || error.message || String(error)) : message;
            __console_native._error(`Uncaught Error: ${errorMsg}`);
        };

        // Handler for unhandled promise rejections
        globalThis.onunhandledrejection = (event) => {
            const reason = event && event.reason;
            let errorMsg;
            if (reason instanceof Error) {
                errorMsg = reason.stack || reason.message || String(reason);
            } else if (typeof reason === 'string') {
                errorMsg = reason;
            } else {
                try {
                    errorMsg = JSON.stringify(reason);
                } catch {
                    errorMsg = String(reason);
                }
            }
            __console_native._error(`Unhandled Promise Rejection: ${errorMsg}`);
        };
    "#,
    )?;

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
        tracing::debug!("SystemJS::get_mods called");

        let mods = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.system_api.get_mods()
        })) {
            Ok(mods) => mods,
            Err(e) => {
                tracing::error!("Panic in get_mods: {:?}", e);
                return Err(rquickjs::Error::Exception);
            }
        };

        tracing::debug!("SystemJS::get_mods got {} mods", mods.len());

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
            obj.set("download_url", mod_info.download_url.as_deref())?;
            array.set(idx, obj)?;
        }

        tracing::debug!("SystemJS::get_mods returning array");
        Ok(array)
    }

    /// Register an event handler
    ///
    /// # Arguments
    /// * `event` - The event type (SystemEvents enum value)
    /// * `handler` - The callback function to invoke
    /// * `priority` - Handler priority (lower numbers execute first)
    /// * `protocol` - Protocol filter string for RequestUri ("stam://", "http://", or "" for all)
    /// * `route` - Route prefix filter for RequestUri
    ///
    /// # Returns
    /// Unique handler ID for later removal
    #[qjs(rename = "register_event")]
    pub fn register_event<'js>(
        &self,
        ctx: Ctx<'js>,
        event: u32,
        handler: Function<'js>,
        priority: i32,
        protocol_str: Option<String>,
        route: Option<String>,
    ) -> rquickjs::Result<u64> {
        // Get the current mod_id from context globals
        let mod_id: String = ctx
            .globals()
            .get("__MOD_ID__")
            .unwrap_or_else(|_| "unknown".to_string());

        tracing::debug!(
            "SystemJS::register_event called: mod={}, event={}, priority={}, protocol={:?}, route={:?}",
            mod_id,
            event,
            priority,
            protocol_str,
            route
        );

        // Validate event type
        let event_type = match SystemEvents::from_u32(event) {
            Some(e) => e,
            None => {
                tracing::error!("Invalid event type: {}", event);
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
    /// - file_content: Uint8Array | null
    #[qjs(rename = "download")]
    pub async fn download<'js>(&self, ctx: Ctx<'js>, uri: String) -> rquickjs::Result<Object<'js>> {
        tracing::debug!("NetworkJS::download called: uri={}", uri);

        // Perform the download
        let response = self.network_api.download(&uri).await;

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
        if let Some(file_name) = response.file_name {
            result.set("file_name", file_name)?;
        } else {
            result.set("file_name", rquickjs::Null)?;
        }

        // Convert file_content to Uint8Array or null
        if let Some(file_content) = response.file_content {
            let array = rquickjs::TypedArray::<u8>::new(ctx.clone(), file_content)?;
            result.set("file_content", array)?;
        } else {
            result.set("file_content", rquickjs::Null)?;
        }

        Ok(result)
    }
}

/// Setup network API in the JavaScript context
///
/// Provides network.download(uri) function that returns a Promise
/// for downloading resources via stam:// protocol.
pub fn setup_network_api(ctx: Ctx, network_api: NetworkApi) -> Result<(), rquickjs::Error> {
    // First, define the class in the runtime (required before creating instances)
    rquickjs::Class::<NetworkJS>::define(&ctx.globals())?;

    // Create an instance of NetworkJS
    let network_obj = rquickjs::Class::<NetworkJS>::instance(ctx.clone(), NetworkJS { network_api })?;

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
