# Staminal Client Assets

This directory contains all client-side assets including localizations, downloaded mods, and cached resources.

## Directory Structure

```
assets/
├── locales/           # Internationalization (i18n) files
│   ├── en-US/        # English (United States)
│   │   └── main.ftl  # Main translation file
│   ├── it-IT/        # Italian (Italy)
│   │   └── main.ftl  # Main translation file
│   └── ...           # Other languages
│
├── mods/             # Downloaded mod files from server (future)
│   └── ...
│
└── cache/            # Cached game assets (future)
    └── ...
```

## Localization System

The client uses Mozilla's [Fluent](https://projectfluent.org/) localization system, which provides:

- **Natural-sounding translations**: Variables, plurals, and gender
- **Fallback system**: Falls back to English if translation is missing
- **Auto-detection**: Automatically detects system locale
- **Manual override**: Can be overridden via `--lang` argument or `STAM_LANG` environment variable
- **Message IDs**: Server sends message IDs, not plain text

### Setting Language

You can specify the language in three ways (in order of priority):

1. **Command-line argument**: `stam_client --lang it-IT`
2. **Environment variable**: `STAM_LANG=it-IT stam_client`
3. **System locale** (auto-detected if not specified)

Example:
```bash
# Use Italian
STAM_LANG=it-IT cargo run

# Or with argument
cargo run -- --lang it-IT

# Use system locale (auto-detect)
cargo run
```

### Adding a New Language

1. Create a new directory under `locales/` with the language tag (e.g., `fr-FR`, `de-DE`)
2. Create a `main.ftl` file in that directory
3. Copy the structure from `en-US/main.ftl` and translate the messages
4. The client will automatically detect and load the new locale

### Example Locale File (Fluent Format)

```fluent
# Simple message
hello = Hello, world!

# Message with variables
welcome = Welcome, {$username}!

# Message with plurals
unread-messages = You have {$count ->
    [one] one unread message
   *[other] {$count} unread messages
}
```

### Using Locales in Code

```rust
// Simple message
let msg = locale.get("hello");

// Message with arguments
use fluent_bundle::FluentArgs;
let mut args = FluentArgs::new();
args.set("username", "Alice");
let msg = locale.get_with_args("welcome", Some(&args));

// Or use the macro
let msg = locale.get_with_args("welcome", Some(&fluent_args!{
    "username" => "Alice"
}));
```

## Server Message IDs

When the server sends a `Disconnect` message, it includes a message ID (not plain text). The client looks up this ID in the current locale files.

Example server-defined message IDs:
- `disconnect-server-shutdown`
- `disconnect-kicked`
- `disconnect-banned`
- `disconnect-idle-timeout`
- `disconnect-version-mismatch`
- `disconnect-maintenance`

This allows the same server message to be displayed in the user's preferred language.

## Workspace Data

The `workspace_data/` directory (sibling to `assets/`) serves as the client's "home" directory:

```
workspace_data/
├── config.json       # Client configuration (future)
├── saves/            # Local save files (future)
└── logs/             # Client logs (future)
```

This separation ensures:
- **assets/** = Read-only client data (locales, initial resources)
- **workspace_data/** = Read-write user data (config, saves, cache)
