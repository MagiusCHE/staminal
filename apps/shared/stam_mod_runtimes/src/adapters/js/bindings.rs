//! JavaScript bindings for runtime APIs
//!
//! This module provides the bridge between Rust APIs and JavaScript contexts.

use rquickjs::{Ctx, Function, Object};
use rquickjs::function::Rest;
use crate::api::{ConsoleApi, AppApi};

/// Setup console API in the JavaScript context
///
/// Provides console.log, console.error, console.warn, console.info, console.debug
/// All functions accept variadic arguments and read the global __MOD_ID__ variable to prefix log messages
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
