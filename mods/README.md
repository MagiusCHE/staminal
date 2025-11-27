# Staminal Mods System

This document describes the mod loading flow for both server and client.

## Mod Structure

```
mods/
└── my-mod/
    ├── manifest.json        # Fallback manifest (used if no side-specific manifest)
    ├── main.js              # Entry point (if shared)
    ├── locale/              # Optional: mod-specific translations
    │   ├── en-US/
    │   │   └── main.ftl
    │   └── it-IT/
    │       └── main.ftl
    ├── client/              # Client-specific code
    │   ├── manifest.json    # Client manifest (overrides root)
    │   └── index.js         # Client entry point
    └── server/              # Server-specific code
        ├── manifest.json    # Server manifest (overrides root)
        └── index.js         # Server entry point
```

## Manifest Resolution

When loading a mod, the system looks for manifests in this order:

1. `mods/{mod-id}/{side}/manifest.json` (side = "client" or "server")
2. `mods/{mod-id}/manifest.json` (fallback)

This allows mods to have different configurations for client and server.

---

## Server Mod Loading Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        initialize_all_games()                                │
│                           (mod_loader.rs)                                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  1. RESOLVE MODS_ROOT                                                       │
│     resolve_mods_root() - Determines base path for mods                     │
│     • If absolute path → use directly                                       │
│     • If custom_home → combine with mods_path                               │
│     • Otherwise → use current_dir + mods_path                               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  2. FOR EACH GAME in config (initialize_game_mods)                          │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
        ┌───────────────────────────┴───────────────────────────┐
        ▼                                                       ▼
┌───────────────────────────┐                   ┌───────────────────────────┐
│ PHASE A: LOAD             │                   │ PHASE A: LOAD             │
│ CLIENT MANIFESTS          │                   │ SERVER MANIFESTS          │
│ (for mods with side=client)│                  │ (for mods with side=server)│
│                           │                   │                           │
│ resolve_manifest():       │                   │ resolve_manifest():       │
│ 1. Check mod_dir/client/  │                   │ 1. Check mod_dir/server/  │
│    manifest.json          │                   │    manifest.json          │
│ 2. Fallback: mod_dir/     │                   │ 2. Fallback: mod_dir/     │
│    manifest.json          │                   │    manifest.json          │
└───────────────────────────┘                   └───────────────────────────┘
        │                                                       │
        ▼                                                       ▼
┌───────────────────────────┐                   ┌───────────────────────────┐
│ PHASE B: VALIDATE         │                   │ PHASE B: VALIDATE         │
│ CLIENT DEPENDENCIES       │                   │ SERVER DEPENDENCIES       │
│                           │                   │                           │
│ validate_mod_dependencies │                   │ validate_mod_dependencies │
│ (stam_schema)             │                   │ (stam_schema)             │
│                           │                   │                           │
│ • Check @server version   │                   │ • Check @server version   │
│ • Check @game version     │                   │ • Check @game version     │
│ • Check mod dependencies  │                   │ • Check mod dependencies  │
│ • SKIP @client (server!)  │                   │ • SKIP @client (server!)  │
└───────────────────────────┘                   └───────────────────────────┘
                                                            │
                                                            ▼
                            ┌─────────────────────────────────────────────────┐
                            │  3. INITIALIZE JS RUNTIME (if server_mods exist)│
                            │                                                 │
                            │  • JsRuntimeConfig (data_dir, config_dir)       │
                            │  • JsRuntimeAdapter::new()                      │
                            │  • LocaleApi (stub with "[id]" fallback)        │
                            └─────────────────────────────────────────────────┘
                                                            │
                                                            ▼
                            ┌─────────────────────────────────────────────────┐
                            │  4. FIRST PASS: REGISTER ALIASES & INFO         │
                            │                                                 │
                            │  For each server_mod:                           │
                            │  • Calculate absolute_entry_point               │
                            │  • register_mod_alias(mod_id, path)             │
                            │  • register_mod_info(ModInfo {...})             │
                            │  • Collect (mod_id, path, mod_type)             │
                            └─────────────────────────────────────────────────┘
                                                            │
                                                            ▼
                            ┌─────────────────────────────────────────────────┐
                            │  5. SECOND PASS: LOAD & ATTACH                  │
                            │                                                 │
                            │  For each mod_entry:                            │
                            │  ┌─────────────────────────────────────────┐    │
                            │  │ runtime_manager.load_mod(mod_id, path) │    │
                            │  │  → Load JS file                         │    │
                            │  │  → Load locale/ if present              │    │
                            │  │  → Execute the module                   │    │
                            │  └─────────────────────────────────────────┘    │
                            │                    │                            │
                            │                    ▼                            │
                            │  ┌─────────────────────────────────────────┐    │
                            │  │ call_mod_function(mod_id, "onAttach")  │    │
                            │  │  → Call mod's exported onAttach()       │    │
                            │  └─────────────────────────────────────────┘    │
                            └─────────────────────────────────────────────────┘
                                                            │
                                                            ▼
                            ┌─────────────────────────────────────────────────┐
                            │  6. THIRD PASS: BOOTSTRAP (only type=bootstrap) │
                            │                                                 │
                            │  For mods with mod_type == "bootstrap":         │
                            │  ┌─────────────────────────────────────────┐    │
                            │  │ call_mod_function(mod_id, "onBootstrap")│    │
                            │  │ system_api.set_bootstrapped(mod_id,true)│    │
                            │  └─────────────────────────────────────────┘    │
                            └─────────────────────────────────────────────────┘
                                                            │
                                                            ▼
                            ┌─────────────────────────────────────────────────┐
                            │  7. RETURN GameModRuntime                       │
                            │                                                 │
                            │  { runtime_manager, js_runtime, server_mods,    │
                            │    client_mods }                                │
                            └─────────────────────────────────────────────────┘
```

---

## Client Mod Loading Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        connect_to_game_server()                              │
│                              (main.rs)                                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  1. RECEIVE MOD LIST FROM SERVER                                            │
│                                                                             │
│  GameMessage::LoginSuccess { mods: Vec<ModInfo> }                           │
│  Contains: mod_id, mod_type, download_url for each required mod             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  2. CHECK LOCAL AVAILABILITY                                                │
│                                                                             │
│  For each mod in server list:                                               │
│  • Check if mods_dir/{mod_id}/ exists                                       │
│  • Check if manifest.json exists                                            │
│  • Load manifest if available                                               │
│  • Track missing mods separately                                            │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  3. INITIALIZE JS RUNTIME                                                   │
│                                                                             │
│  • JsRuntimeConfig (data_dir, config_dir, game_id)                          │
│  • JsRuntimeAdapter::new()                                                  │
│  • LocaleApi with hierarchical lookup (mod locale → global locale)          │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  4. REGISTER MOD ALIASES                                                    │
│                                                                             │
│  For each AVAILABLE mod:                                                    │
│  • Calculate absolute_entry_point                                           │
│  • register_mod_alias(mod_id, path)  → enables @mod-id imports              │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  5. REGISTER MOD INFO (ALL mods, including missing)                         │
│                                                                             │
│  Available mods: full ModInfo from manifest                                 │
│  Missing mods: minimal info (version="?", loaded=false)                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  6. COLLECT DEPENDENCIES (recursive)                                        │
│                                                                             │
│  collect_dependencies():                                                    │
│  • Start from bootstrap mods                                                │
│  • Recursively collect all required dependencies                            │
│  • Detect circular dependencies                                             │
│  • Skip @client, @server, @game requirements                                │
│  • Sort by priority (lower = loads first)                                   │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  7. LOAD & ATTACH (only bootstrap mods + their dependencies)                │
│                                                                             │
│  For each mod in sorted order:                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │ runtime_manager.load_mod(mod_id, path)                             │    │
│  │  → Load JS file                                                     │    │
│  │  → Load locale/ if present (mod-specific translations)              │    │
│  │  → Execute the module                                               │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                    │                                                        │
│                    ▼                                                        │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │ call_mod_function(mod_id, "onAttach")                              │    │
│  │ system_api.set_loaded(mod_id, true)                                │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  8. BOOTSTRAP (only type=bootstrap mods)                                    │
│                                                                             │
│  For each bootstrap mod:                                                    │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │ call_mod_function(mod_id, "onBootstrap")                           │    │
│  │ system_api.set_bootstrapped(mod_id, true)                          │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  Bootstrap mods can then:                                                   │
│  • Download missing mods via system.get_mods()                              │
│  • Load additional mods dynamically                                         │
│  • Show UI for mod management                                               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  9. RUN JS EVENT LOOP                                                       │
│                                                                             │
│  tokio::select! {                                                           │
│    • Handle Ctrl+C                                                          │
│    • Maintain game connection                                               │
│    • Run JS event loop (for timers, async operations)                       │
│  }                                                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Key Differences: Server vs Client

| Aspect | Server | Client |
|--------|--------|--------|
| **Loading** | All mods loaded immediately | Only bootstrap + deps, others deferred |
| **@client validation** | Skipped | Validated |
| **LocaleApi** | Stub returning `[id]` | Global locale manager with fallback |
| **Missing mods** | Fatal error | Can be downloaded by bootstrap mod |
| **mod_info.loaded** | Always `true` | `true` only if actually loaded |
| **Mod source** | Local mods/ directory | Server-provided list + local cache |

---

## Mod Lifecycle Hooks

Each mod can export these functions:

```javascript
// Called when mod is loaded into the runtime
export function onAttach() {
    console.log("Mod attached");
}

// Called only for mods with type="bootstrap"
export function onBootstrap() {
    console.log("Bootstrap started");
}

// Called when mod is being unloaded (not yet implemented)
export function onDetach() {
    console.log("Mod detaching");
}
```

---

## Locale System

Mods can provide their own translations:

```
my-mod/
└── locale/
    ├── en-US/
    │   └── main.ftl
    └── it-IT/
        └── main.ftl
```

The `locale.get()` and `locale.get_with_args()` functions use hierarchical lookup:

1. **Mod's locale** (if present for current language)
2. **Mod's fallback locale** (e.g., en-US)
3. **Global application locale**
4. Returns `[message-id]` if not found

Example usage in JavaScript:

```javascript
// Simple message
const msg = locale.get("welcome-message");

// Message with arguments
const error = locale.get_with_args("download-failed", { mod_id: "my-mod" });
```

Example `main.ftl`:

```ftl
welcome-message = Welcome to the game!
download-failed = Failed to download mod "{$mod_id}"
```
