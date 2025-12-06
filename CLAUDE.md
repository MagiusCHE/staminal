# Instructions and Context for AI Agent

This document serves as a development session summary for the Staminal project (Staminal Engine).

## 1. Project Identity

| Key                       | Value                                 | Description                                                                                                                            |
| :------------------------ | :------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------- |
| **Full Name**             | Staminal                              | Inspired by "stem cells" (undifferentiated).                                                                                           |
| **Goal**                  | Undifferentiated Game Engine Core     | The Engine only provides the platform (networking, VFS, security); game logic is provided by Mods.                                     |
| **Core Language**         | Rust (Latest stable version)          | Chosen for performance and safety (memory management).                                                                                 |
| **Server Platform**       | Linux (Manjaro)                       | The server targets Linux only.                                                                                                         |
| **Client Platforms**      | Linux, macOS (Windows planned)        | The client is cross-platform. Current focus: Linux. Keep portability in mind.                                                          |
| **Dev Environment**       | Linux (Manjaro)                       | Primary development environment.                                                                                                       |
| **Module Prefix**         | `stam_`                               | All internal libraries (crates) use the `stam_` prefix (e.g., `stam_protocol`).                                                        |
| **Key Principle**         | Intent-based Networking               | The client declares the intent (`INTENT: "main:login:survival"`), and the server "differentiates" accordingly, sending necessary Mods. |

## 2. Workspace Structure

The project is organized as follows:

```text
staminal/
├── package.json              (npm scripts for running server/client)
├── apps/
│   ├── stam_server/          (Dedicated Server Binary)
│   │   ├── Cargo.toml
│   │   ├── src/main.rs
│   │   └── workspace_data/   (Server runtime data)
│   │       └── configs/      (Server configuration files)
│   ├── stam_client/          (Game Client Binary)
│   │   ├── Cargo.toml
│   │   ├── src/main.rs
│   │   ├── assets/           (Client assets: locales, etc.)
│   │   └── workspace_data/   (Client runtime data)
│   │       └── demo/mods/    (Downloaded mods per game)
│   └── shared/               (Shared libraries)
│       ├── stam_protocol/    (Network protocol definitions)
│       ├── stam_schema/      (JSON schema validation)
│       └── stam_mod_runtimes/ (Mod runtime engines: JS, etc.)
└── docs/                     (Documentation)
```

## 3. NPM Scripts

Run server and client from the project root:

```bash
npm run server:debug    # Start the server
npm run client:debug    # Start the client
```

## 4. Mod System

### Mod Types
- **bootstrap**: Entry point mods that start game logic (calls `onBootstrap`)
- **library**: Helper mods that provide utilities (only calls `onAttach`)

### Mod Lifecycle
1. Server sends mod list to client on connection
2. Client validates all mods are present locally or downloads them if needed
3. Client registers aliases for ALL mods (`@mod-id` syntax for cross-mod imports)
4. Client loads ALL mods
5. Client calls `onAttach()` for ALL mods
6. Client calls `onBootstrap()` only for bootstrap mods

### Cross-Mod Imports
Mods can import from other mods using the `@mod-id` syntax:
```javascript
import { helper } from '@js-helper';
```

## 5. Logging

- Logs will be output without colors if tty is not detected.
- Use `STAM_LOGDEPS=1` to enable logging from external dependencies (bevy, wgpu, vulkan, naga, winit, etc.). By default (`STAM_LOGDEPS=0`), only Staminal code logs at DEBUG level while external dependencies are filtered to WARN to reduce noise.

## 6. Golden Rules

**IMPORTANT: Read and follow these rules when developing on this project.**

### Code Style
1. **Rust Edition**: Always use `edition = "2024"` in Cargo.toml files
2. **Async Runtime**: Use `tokio` for all async operations
3. **Error Handling**: Use `Result<T, E>` with descriptive error messages, avoid `unwrap()` in production code
4. **Logging**: Use `tracing` crate macros (`info!`, `debug!`, `error!`, etc.)
5. **TODOs as warnings**: Never leave `// TODO:` comments in the code. Instead, use `warn!("TODO: ...")` to log the pending work at runtime. This makes TODOs visible during execution and ensures they are not forgotten.

### Architecture
1. **Separation of Concerns**: The core engine is "undifferentiated" - game logic belongs in Mods, not in the engine
2. **Protocol**: All network communication uses the `stam_protocol` crate's message types
3. **Mod Isolation**: Mods run in sandboxed JavaScript runtime (rquickjs)
4. **No Global State**: Pass configuration and state explicitly, avoid global mutable state

### File Organization
1. **Shared Code**: Put reusable code in `apps/shared/stam_*` crates
2. **Client-specific**: Client-only code stays in `apps/stam_client/src/`
3. **Server-specific**: Server-only code stays in `apps/stam_server/src/`
4. **Common implementations go to shared**: Whenever functionality is needed by both client and server, create a shared library in `apps/shared/` instead of duplicating code. Examples: `stam_protocol` for network messages, `stam_schema` for validation, `stam_mod_runtimes` for mod execution

### Cross-Platform (Client)
1. **Keep portability in mind**: The client must run on Linux, macOS, and eventually Windows
2. **Avoid platform-specific code**: When unavoidable, use `#[cfg(target_os = "...")]` conditionals
3. **File paths**: Use `std::path::Path` and `PathBuf`, never hardcode path separators
4. **Line endings**: Don't assume `\n` - use platform-appropriate methods when needed
5. **Dependencies**: Check that crate dependencies support all target platforms

### Testing & Running
1. **Always build before testing**: `cargo build` in the relevant app directory
2. **ALWAYS use npm scripts**: Use `npm run server:debug` and `npm run client:debug` to run the applications. These scripts set the correct environment variables (`STAM_URI`, `STAM_LANG`, `STAM_HOME`, etc.) needed for proper operation. Never run the binaries directly with `cargo run` as this will miss required configuration.
3. **Check logs**: When debugging, enable `STAM_LOG_FILE=1` to capture full logs
4. **Log file locations**: In development, server and client produce log files in their respective project directories:
   - Server: `apps/stam_server/stam_server.log`
   - Client: `apps/stam_client/stam_client.log`

### Common Pitfalls to Avoid
1. **Don't use `Module::import`** in rquickjs for loading modules - use `Module::declare` + `eval()` instead
2. **Don't store module references** for later function calls - store namespace in globals instead
3. **ANSI colors in logs**: The logging system auto-detects TTY, but always test with file output
4. **Fluent i18n**: The LocaleManager strips Unicode bidi characters automatically
5. **Cross-mod imports**: Aliases must be registered BEFORE loading any mod

### Dependencies
When adding new dependencies:
1. Check if a similar dependency already exists in the workspace
2. Prefer well-maintained crates with active development
3. Enable only necessary features to minimize compile time
4. **Always prefer established crates over custom implementations**: Before implementing any functionality from scratch, search for well-known Rust crates that already solve the problem (e.g., `serde` for serialization, `tokio` for async, `tracing` for logging, `clap` for CLI args). Only implement custom solutions when no suitable crate exists or when there's a specific performance/integration requirement.
5. **ALWAYS use latest versions**: When adding or updating dependencies, ALWAYS use the latest stable version available. Do not use old versions unless there is a specific compatibility requirement. Regularly check and update dependencies to their latest versions. Use `cargo outdated` or check crates.io to verify you're using the most recent version.
6. **Web searches for package documentation**: When searching the web for documentation or examples of crates already in use, ALWAYS first check the version installed in the project's `Cargo.toml` files and search for documentation specific to that version. Do not assume the latest version is in use - verify it first. This prevents issues from API changes between versions.

### JavaScript Global Objects Naming Convention
All global objects exposed to JavaScript mods MUST follow the **PascalCase** naming convention (capitalized):

| Global Object | Description |
|---------------|-------------|
| `System` | System API (mods, events, game info, paths) |
| `Graphic` | Graphic engine API (windows, widgets, fonts) |
| `World` | ECS API (entities, components, queries, systems) |
| `Network` | Network API (downloads) |
| `Locale` | Localization API (translations) |
| `Process` | Process API (app paths, environment) |
| `File` | File API (secure file read/write operations) |
| `Text` | Text utilities |
| `Resource` | Resource loading API (images, fonts, etc.) |

**Exception:** `console` remains lowercase as it follows the standard JavaScript API convention (browser/Node.js).

**Enum constants** are also PascalCase: `SystemEvents`, `GraphicEngines`, `WidgetTypes`, `WindowPositionModes`, `FlexDirection`, `JustifyContent`, `AlignItems`, `ModSides`, `SystemBehaviors`, `FieldTypes`.

**Example usage in mods:**
```javascript
// Correct
const mods = System.getMods();
await Graphic.enableEngine(GraphicEngines.Bevy, config);
const text = Locale.get("welcome");
console.log("Data path:", Process.app.data_path);
const config = File.readJson("settings.json", "utf-8", {});

// Wrong (lowercase - will not work)
const mods = system.getMods();  // ❌
```

### Mod Runtime Development
When developing features for the mod system (`stam_mod_runtimes`):
1. **Language-agnostic design**: Even though JavaScript is currently the only supported runtime, design APIs and interfaces to be language-agnostic
2. **Future runtimes planned**: Rust, C++, Lua, and C# runtimes will be added in the future
3. **Common interface**: All runtime adapters must implement the `RuntimeAdapter` trait consistently
4. **Avoid JS-specific assumptions**: Don't hardcode JavaScript-specific behaviors in the core mod loading logic
5. **Lifecycle hooks**: `onAttach()` and `onBootstrap()` must be implementable in any language
6. **Cross-mod communication**: Design inter-mod APIs that can work across different language runtimes
7. **Avoid busy-loops in event loops**: When implementing event loops for any runtime, NEVER use patterns that poll continuously without yielding (e.g., `loop { check(); }` or `loop { runtime.idle().await; }`). Always use proper async primitives like `tokio::select!` with notification mechanisms (`tokio::sync::Notify`) to wait for events without consuming 100% CPU. This is critical for both client and server applications.
8. **Glue code in external files**: All scripting glue code (JavaScript, Lua, C#, etc.) must be placed in separate files in a `glue/` subdirectory within each runtime adapter folder (e.g., `apps/shared/stam_mod_runtimes/src/adapters/js/glue/`). NEVER embed large blocks of script code as string literals in Rust files. The build.rs script concatenates these files at compile time for embedding.
9. **JavaScript method naming convention**: All methods exposed to JavaScript MUST use camelCase naming. Use the `#[qjs(rename = "methodName")]` attribute in rquickjs bindings to ensure consistency. Examples:
   - `get_mods` → `getMods`
   - `attach_mod` → `attachMod`
   - `send_event` → `sendEvent`
   - `set_resizable` → `setResizable`
   - `enable_graphic_engine` → `enableGraphicEngine`

### Path Security (CRITICAL)
**ALL file access from mods MUST be validated through the path security module.**

Mods are sandboxed and can only access files within permitted directories:
- `data_dir` - The game data directory (contains mods, assets, saves, etc.)
- `config_dir` - The configuration directory (optional, for config files)

**Implementation Requirements:**
1. **Use `path_security` module**: Located at `apps/shared/stam_mod_runtimes/src/api/path_security.rs`
2. **Validate BEFORE any file operation**: Always call `validate_and_resolve_path()` or `is_path_permitted()` before reading, writing, or loading any file
3. **Handle relative paths**: Use `make_absolute()` to convert relative paths to absolute using `data_dir` as base
4. **Return relative paths to Bevy**: When passing paths to Bevy's `AssetServer`, strip the `data_dir` prefix to get relative paths

**Example usage:**
```rust
use crate::api::path_security::{PathSecurityConfig, validate_and_resolve_path, make_absolute};

// Get absolute path
let absolute_path = make_absolute(&user_provided_path, &data_dir);

// Validate the path is within permitted directories
let security_config = PathSecurityConfig::new(&data_dir);
let validated_path = validate_and_resolve_path(&absolute_path, &security_config)?;

// For Bevy assets, get relative path
let relative_path = validated_path.strip_prefix(&data_dir)
    .map(|p| p.to_string_lossy().to_string())
    .unwrap_or_else(|_| validated_path.to_string_lossy().to_string());
```

**Security guarantees:**
- Path traversal attacks (../) are blocked by canonicalization
- Symlinks are followed and the real path is validated
- Absolute paths outside permitted directories are rejected

### Logging Hygiene
- Ensure ANSI color codes are disabled when logs are redirected to files or piped (respect TTY detection and `NO_COLOR`), so log files stay plain and readable with correct mod identifiers.
- **Always check for TTY before using colors**: When implementing any logging or console output that uses colors (including in scripting runtimes), always verify that stdout/stderr is connected to a TTY. Never output ANSI escape codes to files or pipes.

### Client-Only vs Server-Only APIs
When implementing new APIs or methods that are only available on one side (client or server):
1. **Always provide descriptive error messages**: Never let the user see a generic `TypeError: undefined is not a function` or `ReferenceError`. Instead, throw a clear error explaining the limitation.
2. **Pattern for client-only methods**: Check if the required context is available (e.g., `game_info` for client). If not, throw an error with the message: `"<method_name>() is not available on the server. This method is client-only."`
3. **Pattern for server-only methods**: Similarly, throw: `"<method_name>() is not available on the client. This method is server-only."`
4. **Document the limitation**: In the Rust doc comments, clearly mark the method as client-only or server-only.
5. **Example implementation** (see `System.getGameInfo()`):
   ```rust
   match self.system_api.get_game_info() {
       Some(info) => { /* return data */ },
       None => {
           Err(ctx.throw(rquickjs::String::from_str(
               ctx.clone(),
               "System.getGameInfo() is not available on the server. This method is client-only.",
           )?.into()))
       }
   }
   ```

### Documentation Maintenance (CRITICAL)
**Every system modification REQUIRES a documentation review.**

After any change to the system (new features, API changes, architectural modifications):
1. **Review the `docs/` folder**: Check if existing documentation needs to be updated
2. **Create missing documentation**: If a relevant document doesn't exist, create it
3. **Keep docs in sync**: Documentation must always reflect the current state of the system

Documentation structure:
- All documentation files go in the `docs/` directory
- Use Markdown format (`.md` files)
- Name files descriptively (e.g., `mod-api.md`, `networking.md`, `events.md`)

### Script Files (CRITICAL)
**NEVER modify script files (JavaScript, Lua, etc.) unless the user EXPLICITLY requests it.**

Script files in `mods/` directories are authored by the user and must not be touched without explicit permission.

Only modify:
- Rust code (client, server, shared libraries)
- Configuration files when explicitly requested

Mod directories structure:
- Mods are located in `mods/` directories
- Each mod may have `client/` and/or `server/` subdirectories
- Files like `index.js` and other scripts belong to the user
