//! JavaScript Runtime Adapter (QuickJS)
//!
//! Provides JavaScript mod execution using the QuickJS engine via rquickjs.

mod runtime;
mod config;
pub mod bindings;

pub use runtime::JsRuntimeAdapter;
pub use runtime::run_js_event_loop;
pub use runtime::register_mod_alias;
pub use config::JsRuntimeConfig;
