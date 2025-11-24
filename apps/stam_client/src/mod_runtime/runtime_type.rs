/// Runtime type enumeration
///
/// Identifies which scripting runtime a mod uses based on its entry_point file extension

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeType {
    JavaScript,
    // Future runtime types:
    // Lua,
    // CSharp,
    // Rust,
    // Cpp,
}

impl RuntimeType {
    /// Determine runtime type from file extension
    ///
    /// # Arguments
    /// * `path` - Path to the entry point file
    ///
    /// # Returns
    /// The runtime type based on file extension
    ///
    /// # Errors
    /// Returns an error if the extension is not supported
    pub fn from_extension(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let extension = path.extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| format!("No file extension found for: {}", path.display()))?;

        match extension {
            "js" => Ok(RuntimeType::JavaScript),
            // Future extensions:
            // "lua" => Ok(RuntimeType::Lua),
            // "cs" => Ok(RuntimeType::CSharp),
            // "rs" => Ok(RuntimeType::Rust),
            // "cpp" | "cc" | "cxx" => Ok(RuntimeType::Cpp),
            _ => Err(format!("Unsupported runtime type for extension: {}", extension).into()),
        }
    }

    /// Get the human-readable name of this runtime type
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeType::JavaScript => "JavaScript",
            // Future:
            // RuntimeType::Lua => "Lua",
            // RuntimeType::CSharp => "C#",
            // RuntimeType::Rust => "Rust",
            // RuntimeType::Cpp => "C++",
        }
    }
}
