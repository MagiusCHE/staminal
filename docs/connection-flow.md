# Staminal Protocol - Connection Flow

Schema del flusso di connessione basato su `stam_protocol`, `stam_client` e `stam_server`.

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

Utilizzato per ottenere la lista dei server disponibili.

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

Utilizzato per entrare in una sessione di gioco.

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

Dopo un `GameLogin` valido, la connessione passa al Game Stream.

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

In qualsiasi momento, il server può inviare messaggi di errore:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              ERROR HANDLING                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│   PrimalMessage::Error { message }       →  Disconnessione immediata            │
│   PrimalMessage::Disconnect { message }  →  Disconnessione graceful             │
│   GameMessage::Error { message }         →  Errore di gioco                     │
│   GameMessage::Disconnect { message }    →  Disconnessione graceful dal gioco   │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Data Structures

### IntentType (enum)

| Variante      | Descrizione                          |
|---------------|--------------------------------------|
| `PrimalLogin` | Ottiene la lista dei server          |
| `GameLogin`   | Entra in un gioco                    |
| `ServerLogin` | Connessione server-to-server (future)|

### ServerInfo

| Campo     | Tipo     | Esempio                              |
|-----------|----------|--------------------------------------|
| `game_id` | `String` | `"demo"`                             |
| `name`    | `String` | `"Demo Game Server"`                 |
| `uri`     | `String` | `"stam://game.example.com:9999"`     |

### ModInfo

| Campo          | Tipo     | Esempio                                      |
|----------------|----------|----------------------------------------------|
| `mod_id`       | `String` | `"mods-manager"`                             |
| `mod_type`     | `String` | `"bootstrap"`, `"library"`                   |
| `download_url` | `String` | `"stam://server/mods-manager/download"`      |

## Mod Download (via Event System)

Il client riceve `ModInfo` con `download_url` e scarica i mod mancanti tramite l'Event System:

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         MOD DOWNLOAD (via Event System)                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│   Client riceve ModInfo con download_url:                                       │
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

- [`primal_message.rs`](../apps/shared/stam_protocol/src/primal_message.rs) - Definisce `PrimalMessage`, `IntentType`, `ServerInfo`
- [`game_message.rs`](../apps/shared/stam_protocol/src/game_message.rs) - Definisce `GameMessage`, `ModInfo`
- [`primal_client.rs`](../apps/stam_server/src/primal_client.rs) - Gestione lato server del primal handshake
- [`game_client.rs`](../apps/stam_server/src/game_client.rs) - Gestione lato server della game session
- [`main.rs`](../apps/stam_client/src/main.rs) - Implementazione client
