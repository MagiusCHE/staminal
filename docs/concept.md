# Development Plan: "Staminal" Game Engine

## 1. Architectural Vision

The goal is to create a **Host Application (Core)** called **Staminal**. The Core is "undifferentiated": it doesn't know the game rules but serves as a vital substrate for Mods. Its distinctive feature is **Intent-based Networking**.

### The Core manages:

- **Networking**: Secure UDP/TCP sockets with state-based protocol
- **Intent Routing**: The client declares the intent (e.g., `login:survival`), the server mutates behavior accordingly
- **Mod Loading and Verification**: SHA-512 hash
- **Staminal VFS**: Complete filesystem isolation (Sandboxing)

## 2. Technology Stack

### The "Core" (Base)

Written in **Rust**:

- **Safety**: Secure memory management
- **Performance**: Zero-cost abstractions
- **Workspace**: Modular management via Cargo

### Language Integration (Polyglot Runtime)

The server exposes the `Staminal.Core` API:

- **LUA (LuaJIT)**: Fast scripting
- **JavaScript (V8/Node)**: UI and Web Logic
- **C# (.NET Core)**: Complex logic

## 3. "Intent-Based" Logical Workflow

### Scenario: Undifferentiated → Differentiated

#### 1. Initial Connection (State: Stem)

```
Client → Server: HELLO
Server → Client: ACK (Staminal Core v0.1)
```

#### 2. Intent Declaration

```
Client → Server: INTENT: "main:login:survival_mode"
```

#### 3. Specialization

The Server checks if it has the capabilities (Mods) to satisfy the intent:

- **If yes** → Sends Mod Manifest
- **If no** (or if it's an Auth Server) → Sends REDIRECT to another node

## 4. Roadmap

### Phase 1: Staminal Core & Networking (Months 1-2)

- Setup Rust Workspace
- StaminalNet Implementation (TCP/UDP)
- Intent packet parsing

### Phase 2: Mod System (Months 2-3)

- Dynamic DLL/Script loader
- SHA-512 hash calculation

### Phase 3: VFS & Security (Months 3-4)

- StaminalVFS implementation to intercept every IO call

## 5. Folder Structure (Rust Workspace)

**Project Name**: `Staminal`
**Module Prefix**: `stam_`

```
Staminal/
├── Cargo.toml          (Workspace Root)
├── crates/
│   ├── stam_net/       (Networking Lib: TCP/UDP/Intents)
│   ├── stam_vfs/       (Virtual File System: Sandboxing)
│   └── stam_shared/    (Protocol Defs, Crypto, Types)
├── apps/
│   ├── stam_server/    (Dedicated Server Binary)
│   └── stam_client/    (Game Client Binary)
└── data/               (Runtime folder)
    ├── mods/
    └── configs/
```