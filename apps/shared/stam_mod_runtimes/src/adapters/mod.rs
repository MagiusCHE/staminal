//! Runtime Adapters
//!
//! This module contains adapters for different scripting runtimes.
//! Each adapter implements the RuntimeAdapter trait and provides language-specific bindings.

#[cfg(feature = "js")]
pub mod js;

#[cfg(feature = "js")]
pub use js::{
    JsRuntimeAdapter, JsRuntimeConfig,
    run_js_event_loop,
};
