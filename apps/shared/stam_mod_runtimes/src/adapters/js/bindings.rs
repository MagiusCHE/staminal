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

use rquickjs::{Ctx, Function, Object};
use rquickjs::function::Rest;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::collections::HashMap;
use tokio::sync::Notify;
use crate::api::{ConsoleApi, AppApi};

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

    // Create console object
    let console = Object::new(ctx.clone())?;

    // console.log - maps to ConsoleApi::log
    let log_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::log(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console.set("log", log_fn)?;

    // console.error - maps to ConsoleApi::error
    let error_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::error(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console.set("error", error_fn)?;

    // console.warn - maps to ConsoleApi::warn
    let warn_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::warn(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console.set("warn", warn_fn)?;

    // console.info - maps to ConsoleApi::info
    let info_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::info(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console.set("info", info_fn)?;

    // console.debug - maps to ConsoleApi::debug
    let debug_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let game_id: Option<String> = ctx.globals().get("__GAME_ID__").ok();
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::debug(game_id.as_deref(), "js", &mod_id, &message);
    })?;
    console.set("debug", debug_fn)?;

    // Register console object globally
    globals.set("console", console)?;

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
