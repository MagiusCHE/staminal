# Mod Runtime System

## Overview

The modular runtime system allows mods to use different scripting languages (JavaScript, Lua, C#, Rust, C++) transparently. The client automatically determines which runtime to use based on the `entry_point` file extension specified in the mod manifest.

## Architecture

### Main Components

1. **`ModRuntimeManager`** - Manages all runtimes and dispatches calls to the appropriate runtime
2. **`RuntimeAdapter` trait** - Common interface that all runtimes must implement
3. **Language-specific runtimes** - Adapter for each language (e.g., `JsRuntimeAdapter`)
4. **`RuntimeType` enum** - Identifies the runtime type based on file extension

### File Structure

```
src/mod_runtime/
├── mod.rs              # ModRuntimeManager and RuntimeAdapter trait
├── runtime_type.rs     # RuntimeType enum and detection logic
└── js_adapter.rs       # JavaScript/QuickJS adapter
```

## How It Works

### 1. Initialization

When the client starts and connects to a game server:

```rust
// Create the manager
let mut runtime_manager = ModRuntimeManager::new();

// Register the JavaScript runtime (one shared for all JS mods)
let js_runtime = JsRuntime::new(runtime_config)?;
runtime_manager.register_js_runtime(JsRuntimeAdapter::new(js_runtime));

// In the future:
// runtime_manager.register_lua_runtime(...);
// runtime_manager.register_csharp_runtime(...);
```

### 2. Loading Mods

The runtime is automatically selected based on the file extension:

```rust
// The manager automatically determines the runtime from entry_point
runtime_manager.load_mod("my-mod", Path::new("./mods/my-mod/main.js"))?;
// -> Uses JavaScript runtime

runtime_manager.load_mod("another-mod", Path::new("./mods/another-mod/init.lua"))?;
// -> Would use Lua runtime (when implemented)
```

### 3. Calling Functions

Calls to mod functions are completely abstracted:

```rust
// The client doesn't know (and doesn't need to know) which runtime this mod uses
runtime_manager.call_mod_function("my-mod", "onAttach")?;
runtime_manager.call_mod_function("my-mod", "onBootstrap")?;

// With return values
let result = runtime_manager.call_mod_function_with_return("my-mod", "getVersion")?;
match result {
    ModReturnValue::String(s) => println!("Version: {}", s),
    ModReturnValue::Int(i) => println!("Version: {}", i),
    ModReturnValue::Bool(b) => println!("Enabled: {}", b),
    ModReturnValue::None => println!("No return value"),
}
```

## Extension → Runtime Mapping

| Extension | Runtime Type | Status |
|-----------|-------------|---------|
| `.js` | JavaScript (QuickJS) | ✅ Implemented |
| `.lua` | Lua | 🔄 Future |
| `.cs` | C# (Mono/CoreCLR) | 🔄 Future |
| `.rs` | Rust (compiled) | 🔄 Future |
| `.cpp`, `.cc`, `.cxx` | C++ (compiled) | 🔄 Future |

## RuntimeAdapter Trait

All runtimes must implement this trait:

```rust
pub trait RuntimeAdapter {
    /// Load a mod script into this runtime
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str)
        -> Result<(), Box<dyn std::error::Error>>;

    /// Call a function in a mod without return value
    fn call_mod_function(&mut self, mod_id: &str, function_name: &str)
        -> Result<(), Box<dyn std::error::Error>>;

    /// Call a function in a mod with return value
    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>>;
}
```

## Architecture Benefits

### 1. **One Runtime Per Type**
Instead of creating a runtime instance per mod, a single instance is created per language type:
- 5 JavaScript mods → 1 shared JavaScript runtime
- 3 Lua mods → 1 shared Lua runtime
- Saves memory and overhead

### 2. **Dynamic Dispatch**
The client doesn't need to know which runtime a mod uses:
```rust
// Works with any mod type!
runtime_manager.call_mod_function(mod_id, "onAttach")?;
```

### 3. **Extensibility**
Adding a new runtime only requires:
1. Implementing the `RuntimeAdapter` trait
2. Adding the extension in `RuntimeType::from_extension()`
3. Registering the runtime in the manager

### 4. **Type Safety**
Return values are type-safe thanks to the `ModReturnValue` enum:
```rust
pub enum ModReturnValue {
    None,
    String(String),
    Bool(bool),
    Int(i32),
}
```

## Complete Example

### Mod Manifest (manifest.json)
```json
{
    "name": "My JavaScript Mod",
    "version": "1.0.0",
    "entry_point": "main.js",
    "priority": 100
}
```

### Mod Code (main.js)
```javascript
function onAttach() {
    console.log("Mod attached!");
}

function onBootstrap() {
    console.log("Bootstrapping...");
    console.log("Data path:", Process.app.data_path);
}

function getModInfo() {
    return "My Awesome Mod v1.0";
}
```

### Client Code (Rust)
```rust
// Initialize
let mut runtime_manager = ModRuntimeManager::new();
runtime_manager.register_js_runtime(js_adapter);

// Load mod (automatically recognizes .js)
runtime_manager.load_mod("my-mod", Path::new("./mods/my-mod/main.js"))?;

// Call lifecycle hooks
runtime_manager.call_mod_function("my-mod", "onAttach")?;
runtime_manager.call_mod_function("my-mod", "onBootstrap")?;

// Get information
let info = runtime_manager.call_mod_function_with_return("my-mod", "getModInfo")?;
if let ModReturnValue::String(s) = info {
    println!("Mod info: {}", s);
}
```

## Implementing Future Runtimes

### Example: Adding Lua

1. **Create adapter** (`src/mod_runtime/lua_adapter.rs`):
```rust
pub struct LuaRuntimeAdapter {
    runtime: LuaRuntime,
}

impl RuntimeAdapter for LuaRuntimeAdapter {
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str) -> Result<(), Box<dyn Error>> {
        // Load Lua script
    }

    fn call_mod_function(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn Error>> {
        // Call Lua function
    }

    // ...
}
```

2. **Update RuntimeType**:
```rust
match extension {
    "js" => Ok(RuntimeType::JavaScript),
    "lua" => Ok(RuntimeType::Lua),  // <-- Add here
    // ...
}
```

3. **Register in client**:
```rust
let lua_runtime = LuaRuntime::new()?;
runtime_manager.register_lua_runtime(LuaRuntimeAdapter::new(lua_runtime));
```

## Best Practices

1. **Runtime Sharing**: One runtime per language type, not per mod
2. **Error Handling**: All errors are propagated with runtime-specific details
3. **Lifecycle Hooks**: All mods support `onAttach`, `onBootstrap`, etc.
4. **Type Conversion**: Return values are converted to standard Rust types

## Timer System (setTimeout/setInterval)

### Multi-Runtime Safe Architecture

The timer system is designed to work correctly with **multiple simultaneous runtimes** (JavaScript, Lua, C#, etc.).

#### Key Components

1. **`NEXT_TIMER_ID`** (Global AtomicU32)
   - Atomic counter that guarantees unique IDs **across ALL runtimes**
   - If JavaScript creates timers 1, 2, 3 → Lua will get 4, 5, 6 → C# will get 7, 8, 9
   - **No collision possible** between different runtimes

2. **`TIMER_ABORT_HANDLES`** (Global HashMap)
   - Shared registry: `timer_id -> Arc<Notify>`
   - Allows `clearTimeout(id)` to work regardless of which runtime created the timer
   - Thread-safe via `Mutex`

#### Architecture Schema

```
┌─────────────────────────────────────────────────────────────┐
│                     CLIENT PROCESS                           │
├─────────────────────────────────────────────────────────────┤
│  NEXT_TIMER_ID (AtomicU32) ─────────────────────────────────│
│  TIMER_ABORT_HANDLES (Mutex<HashMap>) ──────────────────────│
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │ JsRuntime    │  │ LuaRuntime   │  │ CSharpRuntime│       │
│  │ (mod1.js)    │  │ (mod2.lua)   │  │ (mod3.cs)    │       │
│  │ timer: 1,2,3 │  │ timer: 4,5   │  │ timer: 6,7   │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### JavaScript Implementation (rquickjs)

```rust
// In bindings.rs
static NEXT_TIMER_ID: AtomicU32 = AtomicU32::new(1);

static TIMER_ABORT_HANDLES: LazyLock<Mutex<HashMap<u32, Arc<Notify>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn set_timeout_interval<'js>(
    ctx: Ctx<'js>,
    cb: Function<'js>,
    msec: Option<u64>,
    is_interval: bool,
) -> rquickjs::Result<u32> {
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);
    let delay = msec.unwrap_or(0).max(4); // 4ms min per HTML5 spec

    let abort = Arc::new(Notify::new());
    TIMER_ABORT_HANDLES.lock().unwrap().insert(id, abort.clone());

    ctx.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = abort.notified() => break,
                _ = tokio::time::sleep(Duration::from_millis(delay)) => {
                    cb.call::<(), ()>(()).ok();
                    if !is_interval { break; }
                }
            }
        }
        TIMER_ABORT_HANDLES.lock().unwrap().remove(&id);
    });

    Ok(id)
}
```

#### JavaScript Event Loop

For timers to work, the client must run the JS event loop:

```rust
// In main.rs
if let Some(js_runtime) = js_runtime_handle {
    tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => { /* shutdown */ }
        _ = maintain_game_connection(&mut stream, locale) => { /* connection closed */ }
        _ = run_js_event_loop(js_runtime) => { /* event loop exited */ }
    }
}
```

#### APIs Available to Mods

```javascript
// setTimeout - executes callback after delay
const id = setTimeout(() => {
    console.log("Fired after 1000ms");
}, 1000);

// clearTimeout - cancels a pending timeout
clearTimeout(id);

// setInterval - executes callback every N ms
const intervalId = setInterval(() => {
    console.log("Tick!");
}, 500);

// clearInterval - cancels an interval
clearInterval(intervalId);
```

#### Implementation Notes for New Runtimes

When implementing timers for a new runtime (Lua, C#, etc.), use the public helper functions exposed in `bindings.rs`:

```rust
// Public functions available for all runtimes:
pub fn next_timer_id() -> u32;                                    // Generate unique ID
pub fn clear_timer(timer_id: u32);                                // Cancel timer
pub fn register_timer_abort_handle(timer_id: u32, abort: Arc<Notify>);  // Register handle
pub fn remove_timer_abort_handle(timer_id: u32);                  // Remove handle
```

**Example for Lua adapter:**

```rust
use stam_mod_runtimes::adapters::js::bindings::{
    next_timer_id, register_timer_abort_handle, remove_timer_abort_handle, clear_timer
};
use tokio::sync::Notify;
use std::sync::Arc;

pub fn lua_set_timeout(delay_ms: u64, callback: LuaCallback) -> u32 {
    let id = next_timer_id();  // Globally unique ID

    let abort = Arc::new(Notify::new());
    register_timer_abort_handle(id, abort.clone());

    tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = abort.notified() => { /* cancelled */ }
            _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => {
                callback.call();
            }
        }
        remove_timer_abort_handle(id);
    });

    id
}

pub fn lua_clear_timeout(timer_id: u32) {
    clear_timer(timer_id);  // Works even for JS timers!
}
```

## Current Limitations

1. Only JavaScript is implemented
2. Return values are limited to: None, String, Bool, Int
3. Complex objects or arrays are not yet supported (but possible via JSON)

## Roadmap

- [x] Implement setTimeout/setInterval for JavaScript
- [ ] Implement Lua runtime
- [ ] Implement C# runtime
- [ ] Support complex return values (objects, arrays)
- [ ] Add sandboxing for security
- [ ] Hot-reload mods without restarting the client
