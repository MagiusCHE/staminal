# Instructions and Context for AI Agent

This document serves as a development session summary for the Staminal project (Staminal Engine).

## 1. Project Identity

| Key                       | Value                                 | Description                                                                                                                            |
| :------------------------ | :------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------- |
| **Full Name**             | Staminal                              | Inspired by "stem cells" (undifferentiated).                                                                                           |
| **Goal**                  | Undifferentiated Game Engine Core     | The Engine only provides the platform (networking, VFS, security); game logic is provided by Mods.                                     |
| **Core Language**         | Rust (Latest stable version)          | Chosen for performance and safety (memory management).                                                                                 |
| **Operating System**      | Linux (Manjaro)                       | The primary target environment for development.                                                                                        |
| **Module Prefix**         | `stam_`                               | All internal libraries (crates) use the `stam_` prefix (e.g., `stam_protocol`).                                                             |
| **Key Principle**         | Intent-based Networking               | The client declares the intent (`INTENT: "main:login:survival"`), and the server "differentiates" accordingly, sending necessary Mods. |

## 2. Workspace Structure (Rust/Cargo)

The project is managed as a Rust Workspace. The folder structure is as follows:

```text
Staminal/
├── Cargo.toml (Workspace Root - **UPDATED**)
├── crates/
│   ├── stam_net/      (Networking Lib: TCP/UDP, Session Management)
│   ├── stam_vfs/      (Virtual File System for IO Sandboxing)
│   └── stam_shared/   (Protocol Definitions, Shared Types, Crypto)
├── apps/
│   ├── stam_server/   (Dedicated Server Binary)
│   └── stam_client/   (Game Client Binary)
└── data/              (Runtime Folder)
    ├── mods/          (Where Mod files reside)
    └── configs/
```

## 3. Current Status of Core Files

| File                            | Description                                                    | Last Update                               |
| :------------------------------ | :------------------------------------------------------------- | :---------------------------------------- |
| `Cargo.toml`                    | Defines the workspace with `stam_*` members.                   | Complete                                  |
| `apps/stam_server/src/main.rs`  | Server entry point. Contains the Main Loop (tick rate 64).     | Ready for `stam_net` integration.         |
| `Piano_Sviluppo_Core_Engine.md` | Roadmap and general Architecture.                              | Complete                                  |

## 4. Next Step and Instructions for AI

The next goal is to implement the basic networking library: `crates/stam_net`.

### Instructions:

1.  Create the `crates/stam_net/` folder and the `Cargo.toml` file inside it.
2.  Create the `crates/stam_net/src/lib.rs` file.
3.  Implement a basic `Server` struct with a `bind` function for a UDP (User Datagram Protocol) server, as it's the standard for gamedev (faster than TCP).
4.  The implementation must be a working placeholder that doesn't yet require the dependency on `stam_shared`.

### Code Requirements:

- The `bind` function should return a `Result<Server, std::io::Error>`.
- It must use `std::net::UdpSocket`.

### Next Recommended Command:

"Proceed with creating the crates/stam_net library as specified in the plan, starting with the Server struct implementation."
