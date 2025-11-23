use rquickjs::{Ctx, Function, Object};
use tracing::{info, error, debug, warn};

/// Setup console API in the JavaScript context
///
/// Provides console.log, console.error, console.warn, console.info, console.debug
/// All functions read the global __MOD_ID__ variable to prefix log messages
pub fn setup_console_api(ctx: Ctx) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    // Create console object
    let console = Object::new(ctx.clone())?;

    // console.log - maps to tracing::info
    let log_fn = Function::new(ctx.clone(), |ctx: Ctx, msg: String| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        info!("\"{}\" {}", mod_id, msg);
    })?;
    console.set("log", log_fn)?;

    // console.error - maps to tracing::error
    let error_fn = Function::new(ctx.clone(), |ctx: Ctx, msg: String| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        error!("\"{}\" {}", mod_id, msg);
    })?;
    console.set("error", error_fn)?;

    // console.warn - maps to tracing::warn
    let warn_fn = Function::new(ctx.clone(), |ctx: Ctx, msg: String| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        warn!("\"{}\" {}", mod_id, msg);
    })?;
    console.set("warn", warn_fn)?;

    // console.info - maps to tracing::info (same as log)
    let info_fn = Function::new(ctx.clone(), |ctx: Ctx, msg: String| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        info!("\"{}\" {}", mod_id, msg);
    })?;
    console.set("info", info_fn)?;

    // console.debug - maps to tracing::debug
    let debug_fn = Function::new(ctx.clone(), |ctx: Ctx, msg: String| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        debug!("\"{}\" {}", mod_id, msg);
    })?;
    console.set("debug", debug_fn)?;

    // Register console object globally
    globals.set("console", console)?;

    debug!("Console API registered");
    Ok(())
}
