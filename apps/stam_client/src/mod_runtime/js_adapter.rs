/// JavaScript Runtime Adapter
///
/// Adapts the QuickJS JsRuntime to the RuntimeAdapter trait

use std::path::Path;
use crate::js_runtime::JsRuntime;
use super::{RuntimeAdapter, ModReturnValue};

/// Adapter for JavaScript runtime
pub struct JsRuntimeAdapter {
    runtime: JsRuntime,
}

impl JsRuntimeAdapter {
    /// Create a new JavaScript runtime adapter
    pub fn new(runtime: JsRuntime) -> Self {
        Self { runtime }
    }
}

impl RuntimeAdapter for JsRuntimeAdapter {
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime.load_module(mod_path, mod_id)
    }

    fn call_mod_function(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime.call_function_for_mod(function_name, mod_id)
    }

    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        // Try to get a string return value first
        // In the future, we could inspect the JS value type and convert accordingly
        match self.runtime.call_function_for_mod_string(function_name, mod_id)? {
            Some(s) => Ok(ModReturnValue::String(s)),
            None => Ok(ModReturnValue::None),
        }
    }
}
