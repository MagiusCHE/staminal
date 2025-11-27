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

## 6. Golden Rules

**IMPORTANT: Read and follow these rules when developing on this project.**

### Code Style
1. **Rust Edition**: Always use `edition = "2024"` in Cargo.toml files
2. **Async Runtime**: Use `tokio` for all async operations
3. **Error Handling**: Use `Result<T, E>` with descriptive error messages, avoid `unwrap()` in production code
4. **Logging**: Use `tracing` crate macros (`info!`, `debug!`, `error!`, etc.)

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

### Mod Runtime Development
When developing features for the mod system (`stam_mod_runtimes`):
1. **Language-agnostic design**: Even though JavaScript is currently the only supported runtime, design APIs and interfaces to be language-agnostic
2. **Future runtimes planned**: Rust, C++, Lua, and C# runtimes will be added in the future
3. **Common interface**: All runtime adapters must implement the `RuntimeAdapter` trait consistently
4. **Avoid JS-specific assumptions**: Don't hardcode JavaScript-specific behaviors in the core mod loading logic
5. **Lifecycle hooks**: `onAttach()` and `onBootstrap()` must be implementable in any language
6. **Cross-mod communication**: Design inter-mod APIs that can work across different language runtimes
7. **Avoid busy-loops in event loops**: When implementing event loops for any runtime, NEVER use patterns that poll continuously without yielding (e.g., `loop { check(); }` or `loop { runtime.idle().await; }`). Always use proper async primitives like `tokio::select!` with notification mechanisms (`tokio::sync::Notify`) to wait for events without consuming 100% CPU. This is critical for both client and server applications.

### Logging Hygiene
- Ensure ANSI color codes are disabled when logs are redirected to files or piped (respect TTY detection and `NO_COLOR`), so log files stay plain and readable with correct mod identifiers.
