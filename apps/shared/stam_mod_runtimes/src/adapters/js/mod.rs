//! JavaScript Runtime Adapter (QuickJS)
//!
//! Provides JavaScript mod execution using the QuickJS engine via rquickjs.

mod runtime;
mod config;
mod bindings;

pub use runtime::JsRuntimeAdapter;
pub use config::JsRuntimeConfig;
