# File API (JavaScript)

The `File` global object provides secure file system operations for mods. All file operations validate paths against permitted directories (`data_dir` and `config_dir`) to prevent unauthorized access.

## Security

All file operations enforce path security:

- **Path validation**: All paths (relative or absolute) are validated against permitted directories
- **Path traversal protection**: Attempts to escape permitted directories via `../` are blocked
- **Permitted directories**: Files can only be accessed within `data_dir` (game data) or `config_dir` (configuration)

## Methods Overview

| Method | Availability | Description |
|--------|--------------|-------------|
| `readJson(path, encoding, defaultValue?)` | Client & Server | Read and parse a JSON file |

---

## readJson(path, encoding, defaultValue?)

Read a JSON file and parse it into a JavaScript object.

**Arguments:**
- `path: string` - Path to the JSON file (relative or absolute)
- `encoding: string` - File encoding (only `"utf-8"` is supported)
- `defaultValue?: object | array | null` - Default value to return if file doesn't exist or is empty

**Returns:** `object | array` - The parsed JSON content, or `defaultValue` if file doesn't exist/is empty

**Throws:**
- Error if `path` escapes permitted directories (path traversal attack)
- Error if file contains invalid JSON
- Error if `defaultValue` is a primitive type (string, number, boolean)
- Error if `encoding` is not `"utf-8"`

### Path Resolution

Relative paths are resolved in this order:
1. First, try to resolve against `data_dir` (game data directory)
2. If that fails, try to resolve against `config_dir` (configuration directory)

Absolute paths must be within one of the permitted directories.

### Default Value Behavior

The `defaultValue` parameter:
- Must be an **object**, **array**, **null**, or **undefined** (omitted)
- **Primitive types are NOT allowed** (string, number, boolean will throw an error)
- If omitted or `undefined`, defaults to `{}` (empty object)
- Is returned when:
  - File does not exist
  - File exists but is empty
  - File contains only whitespace

### Examples

**Basic usage - read config with empty default:**
```javascript
const config = File.readJson("settings.json", "utf-8", {});
console.log("Settings:", config);
```

**Read with default values:**
```javascript
const config = File.readJson("game-settings.json", "utf-8", {
    volume: 50,
    fullscreen: false,
    language: "en"
});

// If file doesn't exist, config will be:
// { volume: 50, fullscreen: false, language: "en" }
```

**Read from config directory using absolute path:**
```javascript
const configPath = System.getGameConfigPath("saves/slot1.json");
const saveData = File.readJson(configPath, "utf-8", {
    level: 1,
    score: 0,
    items: []
});
```

**Read an array:**
```javascript
const highScores = File.readJson("highscores.json", "utf-8", []);
// Returns [] if file doesn't exist
```

**Error handling:**
```javascript
try {
    const data = File.readJson("config.json", "utf-8", {});
    console.log("Loaded:", data);
} catch (e) {
    console.error("Failed to read config:", e.message);
    // Possible errors:
    // - "Access denied: path '...' escapes permitted directories"
    // - "Invalid JSON in file '...': ..."
    // - "File.readJson() default_value must be an object, array, null, or undefined"
}
```

**Invalid - primitive default (will throw):**
```javascript
// These will throw an error!
File.readJson("config.json", "utf-8", "default");  // string not allowed
File.readJson("config.json", "utf-8", 42);         // number not allowed
File.readJson("config.json", "utf-8", true);       // boolean not allowed
```

**Path traversal (will throw):**
```javascript
// This will throw an error!
try {
    File.readJson("../../../etc/passwd", "utf-8", {});
} catch (e) {
    // "Access denied: path '../../../etc/passwd' escapes the permitted directory"
}
```

---

## Common Patterns

### Loading Game Configuration

```javascript
export function onAttach() {
    // Load user preferences
    const userPrefs = File.readJson("user-preferences.json", "utf-8", {
        musicVolume: 100,
        sfxVolume: 100,
        showTutorials: true
    });

    applyUserPreferences(userPrefs);
}
```

### Loading Save Data

```javascript
async function loadGame(slotNumber) {
    const savePath = System.getGameConfigPath(`saves/slot${slotNumber}.json`);

    const saveData = File.readJson(savePath, "utf-8", null);

    if (saveData === null) {
        console.log("No save data found, starting new game");
        return createNewGame();
    }

    return saveData;
}
```

### Loading Mod-Specific Data

```javascript
// Load mod configuration from data directory
const modConfig = File.readJson("mods/my-mod/config.json", "utf-8", {
    enabled: true,
    debugMode: false
});
```

---

## Error Messages

| Error | Cause |
|-------|-------|
| `"Access denied: path '...' escapes the permitted directory"` | Path traversal attempt or absolute path outside permitted directories |
| `"Invalid JSON in file '...': ..."` | File contains malformed JSON |
| `"File.readJson() default_value must be an object, array, null, or undefined. Primitive types (string, number, boolean) are not allowed."` | Passed a primitive as default value |
| `"Unsupported encoding '...'. Only 'utf-8' is currently supported."` | Used an encoding other than "utf-8" |
| `"Path '...' is not a file"` | Path points to a directory instead of a file |

---

## See Also

- [System API](./system-api.md) - `System.getGameConfigPath()` for getting config file paths
- [Process API](./system-api.md#process) - `Process.app.data_path` and `Process.app.config_path` for directory paths
