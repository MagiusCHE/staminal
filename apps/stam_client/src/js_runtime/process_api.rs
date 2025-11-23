use rquickjs::{Ctx, Object};
use std::path::PathBuf;
use crate::runtime_api::AppApi;

/// Setup process API in the JavaScript context
///
/// Provides process.app.data_path and process.app.config_path as string properties
///
/// This is a JavaScript binding for the runtime-agnostic ProcessApi/AppApi
pub fn setup_process_api(ctx: Ctx, data_dir: PathBuf, config_dir: PathBuf) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    // Create app API instance
    let app_api = AppApi::new(data_dir, config_dir);

    // Create process object
    let process = Object::new(ctx.clone())?;

    // Create process.app object
    let app = Object::new(ctx.clone())?;

    // Set process.app.data_path as a string property
    app.set("data_path", app_api.data_path())?;

    // Set process.app.config_path as a string property
    app.set("config_path", app_api.config_path())?;

    // Register app object to process
    process.set("app", app)?;

    // Register process object globally
    globals.set("process", process)?;

    Ok(())
}
