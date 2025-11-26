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

use crate::api::{AppApi, ConsoleApi, SystemApi};
use rquickjs::{Array, Ctx, Function, Object, class::Trace};
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
#[derive(Clone, Trace)]
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
            array.set(idx, obj)?;
        }

        tracing::debug!("SystemJS::get_mods returning array");
        Ok(array)
    }
}

/// Setup system API in the JavaScript context
///
/// Provides system.get_mods() function that returns an array of mod info objects.
/// Each mod info object contains: id, version, name, description, mod_type, priority, bootstrapped
pub fn setup_system_api(ctx: Ctx, system_api: SystemApi) -> Result<(), rquickjs::Error> {
    // First, define the class in the runtime (required before creating instances)
    rquickjs::Class::<SystemJS>::define(&ctx.globals())?;

    // Create an instance of SystemJS
    let system_obj = rquickjs::Class::<SystemJS>::instance(ctx.clone(), SystemJS { system_api })?;

    // Register it as global 'system' object
    ctx.globals().set("system", system_obj)?;

    Ok(())
}
