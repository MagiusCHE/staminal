use rquickjs::{Ctx, Function, Object};
use rquickjs::function::Rest;
use crate::runtime_api::ConsoleApi;

/// Setup console API in the JavaScript context
///
/// Provides console.log, console.error, console.warn, console.info, console.debug
/// All functions accept variadic arguments and read the global __MOD_ID__ variable to prefix log messages
///
/// This is a JavaScript binding for the runtime-agnostic ConsoleApi
pub fn setup_console_api(ctx: Ctx) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    // Create console object
    let console = Object::new(ctx.clone())?;

    // console.log - maps to ConsoleApi::log
    let log_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::log(&mod_id, &message);
    })?;
    console.set("log", log_fn)?;

    // console.error - maps to ConsoleApi::error
    let error_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::error(&mod_id, &message);
    })?;
    console.set("error", error_fn)?;

    // console.warn - maps to ConsoleApi::warn
    let warn_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::warn(&mod_id, &message);
    })?;
    console.set("warn", warn_fn)?;

    // console.info - maps to ConsoleApi::info
    let info_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::info(&mod_id, &message);
    })?;
    console.set("info", info_fn)?;

    // console.debug - maps to ConsoleApi::debug
    let debug_fn = Function::new(ctx.clone(), |ctx: Ctx, args: Rest<String>| {
        let mod_id: String = ctx.globals().get("__MOD_ID__").unwrap_or_else(|_| "unknown".to_string());
        let message = args.0.join(" ");
        ConsoleApi::debug(&mod_id, &message);
    })?;
    console.set("debug", debug_fn)?;

    // Register console object globally
    globals.set("console", console)?;

    Ok(())
}
