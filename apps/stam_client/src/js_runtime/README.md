# JavaScript Runtime Integration

This directory contains the JavaScript runtime integration for the Staminal client.

## Structure

```
js_runtime/
├── mod.rs           - Module exports and runtime type selection
├── runtime.rs       - Main JsRuntime implementation (QuickJS)
├── console_api.rs   - Console API (console.log, error, warn, etc.)
└── README.md        - This file
```

## Architecture

### Runtime (`runtime.rs`)

The `JsRuntime` struct provides the main interface for loading and executing JavaScript mods:

- `new()` - Initialize QuickJS runtime and register all APIs
- `load_module(path, mod_id)` - Load a JavaScript file and execute it
- `call_function(name)` - Call a global JavaScript function
- `setup_global_apis()` - Register all available APIs

### APIs

Each API is implemented in a separate file for modularity:

#### Console API (`console_api.rs`)

Provides standard console logging functions that bridge to Rust's tracing system:

- `console.log(msg)` → `tracing::info!("{mod_id}: {msg}")`
- `console.error(msg)` → `tracing::error!("{mod_id}: {msg}")`
- `console.warn(msg)` → `tracing::warn!("{mod_id}: {msg}")`
- `console.info(msg)` → `tracing::info!("{mod_id}: {msg}")`
- `console.debug(msg)` → `tracing::debug!("{mod_id}: {msg}")`

All console functions automatically prefix messages with the current mod ID by reading the global `__MOD_ID__` variable.

## Adding New APIs

To add a new API (e.g., `client_api`):

1. Create a new file: `src/js_runtime/client_api.rs`
2. Implement a `setup_client_api(ctx: Ctx)` function
3. Register it in `runtime.rs` → `setup_global_apis()`
4. Export it in `mod.rs` if needed externally

Example:

```rust
// client_api.rs
use rquickjs::{Ctx, Function, Object};

pub fn setup_client_api(ctx: Ctx) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();
    let client = Object::new(ctx.clone())?;

    // Add client.send() function
    let send_fn = Function::new(ctx.clone(), |msg: String| {
        // Implementation
    })?;
    client.set("send", send_fn)?;

    globals.set("client", client)?;
    Ok(())
}
```

Then in `runtime.rs`:

```rust
use super::{console_api, client_api};

fn setup_global_apis(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    self.context.with(|ctx| {
        console_api::setup_console_api(ctx.clone())?;
        client_api::setup_client_api(ctx.clone())?;  // Add this
        Ok::<(), rquickjs::Error>(())
    })?;
    Ok(())
}
```

## Future: Multiple Runtime Support

The structure is designed to support multiple JavaScript runtimes in the future:

```rust
// Future: mod.rs
pub enum RuntimeType {
    QuickJS,
    V8,
}

pub fn create_runtime(runtime_type: RuntimeType) -> Box<dyn JsRuntimeTrait> {
    match runtime_type {
        RuntimeType::QuickJS => Box::new(QuickJsRuntime::new()),
        RuntimeType::V8 => Box::new(V8Runtime::new()),
    }
}
```

Mods will be able to specify their preferred runtime in `manifest.json`:

```json
{
    "name": "my-mod",
    "runtime": "quickjs",  // or "v8"
    "entry_point": "main.js"
}
```

## Testing

To test the runtime integration, run the client with a bootstrap mod:

```bash
STAM_URI="stam://test:test@localhost:9999" \
STAM_GAME="demo" \
STAM_HOME="./workspace_data" \
STAM_LANG="it-IT" \
cargo run
```

Expected output:
```
INFO JavaScript runtime initialized successfully
INFO Loading bootstrap mods...
INFO   Loading mods-manager (main.js)
INFO Mod 'mods-manager' loaded successfully
INFO Attaching bootstrap mods...
INFO mods-manager: SimpleGUI mod attached.
INFO Bootstrap mods attached successfully
```
