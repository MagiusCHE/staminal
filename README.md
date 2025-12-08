# Staminal

**An Undifferentiated Game Engine Core**

Staminal is a modular game engine written in Rust where the core provides only the platform (networking, virtual filesystem, security) while all game logic is delivered through **Mods**. Inspired by stem cells, the engine remains "undifferentiated" — it doesn't dictate what kind of game you build.

## Key Features

### Intent-Based Networking
Clients declare their intent (e.g., `INTENT: "main:login:survival"`), and the server responds by sending the necessary mods. This allows a single server to host multiple game modes or entirely different games.

### Multi-Language Mod System
Mods are sandboxed scripts that define game logic. Currently supported:
- **JavaScript** (QuickJS runtime)

Planned runtimes: Lua, C#, Rust, C++

### Two Mod Types
- **Bootstrap mods**: Entry points that start game logic (`onBootstrap()`)
- **Library mods**: Utility modules imported by other mods (`onAttach()` only)

### Cross-Mod Imports
Mods can import from each other using the `@mod-id` syntax:
```javascript
import { helper } from '@my-utility-mod';
```

### ECS Architecture
Built on [Bevy](https://bevyengine.org/), the engine exposes a complete Entity-Component-System API to mods:
- Spawn entities with components
- Query entities by component filters
- Declare systems with built-in behaviors (gravity, velocity, follow, orbit, etc.)
- Define custom components with JSON schemas
- Use formula-based systems with mathematical expressions

### Graphics & UI
- Window management (create, resize, fullscreen, multi-window)
- UI nodes (text, images, buttons with hover/pressed states)
- Font loading and text rendering
- Per-window camera system

### Security
- Path validation prevents directory traversal attacks
- Mods can only access permitted directories (`data_dir`, `config_dir`)
- Symlinks are resolved and validated

## Architecture

```
staminal/
├── apps/
│   ├── stam_server/           # Dedicated server (Linux)
│   ├── stam_client/           # Game client (Linux, macOS, Windows planned)
│   └── shared/
│       ├── stam_protocol/     # Network protocol definitions
│       ├── stam_schema/       # JSON schema validation
│       └── stam_mod_runtimes/ # Mod runtime engines
├── mods/                      # Game mods
└── docs/                      # Documentation
```

## Development Setup

### Prerequisites
- Rust (latest stable)
- Node.js (for npm scripts, it will removed in favor of cargo scripts in the near future)

### Running the Engine

Always use npm scripts to run the server and client — they set required environment variables:

```bash
# Install dependencies
npm install

# Start the server
npm run server:debug

# Start the client (in another terminal)
npm run client:debug
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `STAM_URI` | Server connection URI (`stam://user:pass@host:port`) |
| `STAM_LANG` | Locale code (e.g., `en-US`, `it-IT`) |
| `STAM_HOME` | Data directory path |
| `STAM_GAME` | Game identifier |
| `STAM_LOG_LEVEL` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `STAM_LOGDEPS` | Enable external dependency logs (`0` or `1`) |

### Log Files

During development, logs are written to:
- Server: `apps/stam_server/stam_server.log`
- Client: `apps/stam_client/stam_client.log`

## JavaScript API Overview

Mods have access to these global objects:

| Object | Purpose |
|--------|---------|
| `System` | Mod info, events, game context, lifecycle |
| `Graphic` | Window management, graphic engine control |
| `World` | ECS operations (spawn, query, systems) |
| `File` | Secure file read/write |
| `Locale` | Localization and translations |
| `Resource` | Asset loading (images, fonts) |
| `Process` | Application paths and environment |
| `console` | Standard logging |

### Example: Creating a Button

```javascript
const button = await World.spawn({
    Node: { width: 200, height: 50 },
    Button: true,
    BackgroundColor: "#4A90D9",
    Text: "Click Me"
});

await button.on("click", () => {
    console.log("Button clicked!");
});
```

## Documentation

See the [docs/](docs/) folder for detailed documentation:
- [Mod Runtime System](docs/mod-runtime-system.md)
- [Event System](docs/events.md)
- [ECS API](docs/mods/js/graphic/ecs.md)
- [Window Management](docs/mods/js/graphic/window.md)
- [File API](docs/mods/js/file.md)

## License

This project is licensed under **CC BY-NC-SA 4.0** (Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International).

See [LICENSE.md](LICENSE.md) for details.
