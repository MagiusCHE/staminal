# Runtime API Abstraction Layer

This directory contains runtime-agnostic API implementations that can be exposed to different scripting runtimes (JavaScript, Lua, C#, Python, etc.).

## Architecture

```
┌─────────────────────────────────────────────────────┐
│               Runtime API (Rust)                    │
│                                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐         │
│  │ Console  │  │ Process  │  │  Future  │         │
│  │   API    │  │   API    │  │   APIs   │         │
│  └──────────┘  └──────────┘  └──────────┘         │
└─────────────────────────────────────────────────────┘
         │              │              │
         ▼              ▼              ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│  JavaScript  │ │     Lua      │ │      C#      │
│   Bindings   │ │   Bindings   │ │   Bindings   │
└──────────────┘ └──────────────┘ └──────────────┘
         │              │              │
         ▼              ▼              ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│   QuickJS    │ │    LuaJIT    │ │  .NET Core   │
│   Runtime    │ │   Runtime    │ │   Runtime    │
└──────────────┘ └──────────────┘ └──────────────┘
```

## Philosophy

Each API is implemented **once** in Rust as a runtime-agnostic module. Then, each scripting runtime creates lightweight bindings that expose these APIs to their specific language.

This approach ensures:
- **Consistency**: All runtimes have identical API behavior
- **Maintainability**: Bug fixes and features added in one place
- **Type Safety**: Rust's type system ensures correctness
- **Performance**: Direct Rust implementation, minimal overhead

## Available APIs

### Console API (`console.rs`)

Provides logging functionality that bridges to Rust's tracing system.

**Rust Implementation:**
```rust
pub struct ConsoleApi;

impl ConsoleApi {
    pub fn log(mod_id: &str, message: &str);
    pub fn error(mod_id: &str, message: &str);
    pub fn warn(mod_id: &str, message: &str);
    pub fn info(mod_id: &str, message: &str);
    pub fn debug(mod_id: &str, message: &str);
}
```

**JavaScript Usage:**
```javascript
console.log("Hello from mod!");
console.error("Something went wrong");
```

**Lua Usage (future):**
```lua
console.log("Hello from mod!")
console.error("Something went wrong")
```

**C# Usage (future):**
```csharp
Console.Log("Hello from mod!");
Console.Error("Something went wrong");
```

### Process API (`process.rs`)

Provides access to process and application information.

**Rust Implementation:**
```rust
pub struct ProcessApi {
    data_dir: PathBuf,
}

pub struct AppApi {
    process_api: ProcessApi,
}

impl AppApi {
    pub fn data_path(&self) -> String;
}
```

**JavaScript Usage:**
```javascript
let dataPath = process.app.data_path();
console.log("Data directory: " + dataPath);
```

**Lua Usage (future):**
```lua
local dataPath = process.app.data_path()
print("Data directory: " .. dataPath)
```

**C# Usage (future):**
```csharp
string dataPath = Process.App.DataPath();
Console.WriteLine($"Data directory: {dataPath}");
```

## Structure

```
runtime_api/
├── mod.rs       - Module exports
├── console.rs   - Console API implementation
├── process.rs   - Process/App API implementation
└── README.md    - This file
```

## Adding a New API

To add a new API (e.g., `client_api`):

### 1. Create Rust Implementation

Create `src/runtime_api/client_api.rs`:

```rust
use std::sync::Arc;

pub struct ClientApi {
    // Internal state if needed
}

impl ClientApi {
    pub fn new() -> Self {
        Self {}
    }

    pub fn send_message(&self, message: &str) -> Result<(), String> {
        // Implementation
        Ok(())
    }

    pub fn get_player_name(&self) -> String {
        "Player".to_string()
    }
}
```

### 2. Export from mod.rs

Update `src/runtime_api/mod.rs`:

```rust
pub mod console;
pub mod process;
pub mod client;  // Add this

pub use console::ConsoleApi;
pub use process::{ProcessApi, AppApi};
pub use client::ClientApi;  // Add this
```

### 3. Create JavaScript Bindings

Create `src/js_runtime/client_api.rs`:

```rust
use rquickjs::{Ctx, Function, Object};
use crate::runtime_api::ClientApi;

pub fn setup_client_api(ctx: Ctx) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();
    let client = Object::new(ctx.clone())?;

    // Create shared API instance
    let api = ClientApi::new();

    // Bind methods
    let send_fn = Function::new(ctx.clone(), move |msg: String| {
        api.send_message(&msg).ok();
    })?;
    client.set("send", send_fn)?;

    let name_fn = Function::new(ctx.clone(), move || {
        api.get_player_name()
    })?;
    client.set("getPlayerName", name_fn)?;

    globals.set("client", client)?;
    Ok(())
}
```

### 4. Register in Runtime

Update `src/js_runtime/runtime.rs`:

```rust
use super::{console_api, process_api, client_api};  // Add client_api

fn setup_global_apis(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    self.context.with(|ctx| {
        console_api::setup_console_api(ctx.clone())?;
        process_api::setup_process_api(ctx.clone(), data_dir)?;
        client_api::setup_client_api(ctx.clone())?;  // Add this
        Ok::<(), rquickjs::Error>(())
    })?;
    Ok(())
}
```

### 5. Future: Add Lua Bindings

Create `src/lua_runtime/client_api.rs`:

```rust
use mlua::{Lua, Table, Function};
use crate::runtime_api::ClientApi;

pub fn setup_client_api(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    let client = lua.create_table()?;

    let api = ClientApi::new();

    client.set("send", lua.create_function(move |_, msg: String| {
        api.send_message(&msg).ok();
        Ok(())
    })?)?;

    client.set("getPlayerName", lua.create_function(move |_, ()| {
        Ok(api.get_player_name())
    })?)?;

    globals.set("client", client)?;
    Ok(())
}
```

## Benefits of This Architecture

### For JavaScript Mods
```javascript
// Same API across all runtimes
let path = process.app.data_path();
console.log("Path: " + path);
```

### For Lua Mods (future)
```lua
-- Identical functionality, Lua syntax
local path = process.app.data_path()
console.log("Path: " .. path)
```

### For C# Mods (future)
```csharp
// Identical functionality, C# syntax
string path = Process.App.DataPath();
Console.Log($"Path: {path}");
```

## Testing

Each API implementation should have Rust unit tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_data_path() {
        let api = AppApi::new(PathBuf::from("/test/path"));
        assert_eq!(api.data_path(), "/test/path");
    }
}
```

## Future APIs

Planned APIs to add:

- **Client API**: Send messages, get player info, etc.
- **Events API**: Subscribe to game events
- **UI API**: Create and manage UI elements
- **Audio API**: Play sounds and music
- **Storage API**: Persistent mod data storage
- **Network API**: HTTP requests for mod updates

Each will follow the same pattern: Rust implementation → Runtime bindings.
