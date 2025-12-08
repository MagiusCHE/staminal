# Staminal Protocol - Connection Flow

Schema of the connection flow based on `stam_protocol`, `stam_client` and `stam_server`.

## Overview

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                    STAMINAL PROTOCOL - CONNECTION FLOW                          │
└─────────────────────────────────────────────────────────────────────────────────┘

┌──────────────┐                                              ┌──────────────┐
│    CLIENT    │                                              │    SERVER    │
└──────────────┘                                              └──────────────┘
       │                                                             │
       │                    TCP CONNECTION                           │
       │═══════════════════════════════════════════════════════════>│
       │                                                             │
```

## Phase 1: Primal Stream (Initial Handshake)

```
╔═════════════════════════════════════════════════════════════════════════════════╗
║                      PRIMAL STREAM (Initial Handshake)                          ║
╚═════════════════════════════════════════════════════════════════════════════════╝
       │                                                             │
       │        ┌─────────────────────────────────────────────────┐ │
       │        │ PrimalMessage::Welcome                          │ │
       │<───────│   • version: "0.1.0"                            │─│
       │        └─────────────────────────────────────────────────┘ │
       │                                                             │
```

## Flow A: PrimalLogin (Server List)

Used to obtain the list of available servers.

```
       ├─────────────────────────────────────────────────────────────┤
       │              FLOW A: PrimalLogin (Server List)              │
       ├─────────────────────────────────────────────────────────────┤
       │                                                             │
       │  ┌─────────────────────────────────────────────────┐       │
       │  │ PrimalMessage::Intent                           │       │
       │──│   • intent_type: IntentType::PrimalLogin        │──────>│
       │  │   • client_version: "0.1.0"                     │       │
       │  │   • username: "magius"                          │       │
       │  │   • password_hash: "sha512:..."                 │       │
       │  │   • game_id: None                               │       │
       │  └─────────────────────────────────────────────────┘       │
       │                                                             │
       │        ┌─────────────────────────────────────────────────┐ │
       │        │ PrimalMessage::ServerList                       │ │
       │<───────│   • servers: Vec<ServerInfo>                    │─│
       │        │     [{ game_id, name, uri }, ...]               │ │
       │        └─────────────────────────────────────────────────┘ │
       │                                                             │
       │                  (Connection closes)                        │
       │                                                             │
```

## Flow B: GameLogin (Game Session)

Used to enter a game session.

```
       ├─────────────────────────────────────────────────────────────┤
       │              FLOW B: GameLogin (Game Session)               │
       ├─────────────────────────────────────────────────────────────┤
       │                                                             │
       │  ┌─────────────────────────────────────────────────┐       │
       │  │ PrimalMessage::Intent                           │       │
       │──│   • intent_type: IntentType::GameLogin          │──────>│
       │  │   • client_version: "0.1.0"                     │       │
       │  │   • username: "magius"                          │       │
       │  │   • password_hash: "sha512:..."                 │       │
       │  │   • game_id: Some("demo")                       │       │
       │  └─────────────────────────────────────────────────┘       │
       │                                                             │
```

## Phase 2: Game Stream

After a valid `GameLogin`, the connection transitions to the Game Stream.

```
╔═════════════════════════════════════════════════════════════════════════════════╗
║                        GAME STREAM (After GameLogin)                            ║
╚═════════════════════════════════════════════════════════════════════════════════╝
       │                                                             │
       │        ┌─────────────────────────────────────────────────┐ │
       │        │ GameMessage::LoginSuccess                       │ │
       │<───────│   • game_name: "Demo Game"                      │─│
       │        │   • game_version: "1.0.0"                       │ │
       │        │   • mods: Vec<ModInfo>                          │ │
       │        │     [{ mod_id, mod_type, download_url }, ...]   │ │
       │        └─────────────────────────────────────────────────┘ │
       │                                                             │
       │  (Client checks mods, downloads missing ones via HTTP)     │
       │                                                             │
       │  ┌─────────────────────────────────────────────────┐       │
       │  │ (Future) GameMessage::Ready or similar          │       │
       │──│   • Client ready to play                        │──────>│
       │  └─────────────────────────────────────────────────┘       │
       │                                                             │
       │        ┌─────────────────────────────────────────────────┐ │
       │        │ GameMessage::Disconnect                         │ │
       │<───────│   • message: "reason"                           │─│
       │        └─────────────────────────────────────────────────┘ │
       │                                                             │
```

## Error Handling

At any time, the server can send error messages:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              ERROR HANDLING                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│   PrimalMessage::Error { message }       →  Immediate disconnection             │
│   PrimalMessage::Disconnect { message }  →  Graceful disconnection              │
│   GameMessage::Error { message }         →  Game error                          │
│   GameMessage::Disconnect { message }    →  Graceful disconnection from game    │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Data Structures

### IntentType (enum)

| Variant       | Description                           |
|---------------|---------------------------------------|
| `PrimalLogin` | Get the server list                   |
| `GameLogin`   | Enter a game                          |
| `ServerLogin` | Server-to-server connection (future)  |

### ServerInfo

| Field     | Type     | Example                              |
|-----------|----------|--------------------------------------|
| `game_id` | `String` | `"demo"`                             |
| `name`    | `String` | `"Demo Game Server"`                 |
| `uri`     | `String` | `"stam://game.example.com:9999"`     |

### ModInfo

| Field          | Type     | Example                                      |
|----------------|----------|----------------------------------------------|
| `mod_id`       | `String` | `"mods-manager"`                             |
| `mod_type`     | `String` | `"bootstrap"`, `"library"`                   |
| `download_url` | `String` | `"stam://server/mods-manager/download"`      |

## Mod Download (via Event System)

The client receives `ModInfo` with `download_url` and downloads missing mods via the Event System:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         MOD DOWNLOAD (via Event System)                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│   Client receives ModInfo with download_url:                                    │
│   "stam://localhost:9999/mods-manager/download?mod_id=some-mod"                 │
│                                                                                 │
│   ┌──────────┐                                              ┌──────────────┐   │
│   │  CLIENT  │  ─── HTTP/STAM Request ──────────────────>   │    SERVER    │   │
│   └──────────┘      GET /mods-manager/download?mod_id=...   └──────────────┘   │
│                                                                    │            │
│                                                         EventDispatcher         │
│                                                         get_handlers_for_       │
│                                                         uri_request()           │
│                                                                    │            │
│                                                                    ▼            │
│                                                          ┌─────────────────┐   │
│                                                          │  mods-manager   │   │
│                                                          │  handler        │   │
│                                                          │  priority=100   │   │
│                                                          │  route=/mods-   │   │
│                                                          │  manager/       │   │
│                                                          └─────────────────┘   │
│                                                                    │            │
│   ┌──────────┐                                                     │            │
│   │  CLIENT  │  <─── Response (mod archive) ───────────────────────┘            │
│   └──────────┘                                                                  │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Source Files

- [`primal_message.rs`](../apps/shared/stam_protocol/src/primal_message.rs) - Defines `PrimalMessage`, `IntentType`, `ServerInfo`
- [`game_message.rs`](../apps/shared/stam_protocol/src/game_message.rs) - Defines `GameMessage`, `ModInfo`
- [`primal_client.rs`](../apps/stam_server/src/primal_client.rs) - Server-side handling of the primal handshake
- [`game_client.rs`](../apps/stam_server/src/game_client.rs) - Server-side handling of the game session
- [`main.rs`](../apps/stam_client/src/main.rs) - Client implementation
