# Locale API (JavaScript)

The `Locale` global object provides internationalization (i18n) support for mods using the [Fluent](https://projectfluent.org/) localization system.

## Overview

The Locale API implements a **hierarchical lookup system**:

1. First checks the **current mod's locale files** (if present)
2. Falls back to the **global application locale**

This allows mods to provide their own translations while also using shared application strings.

## Methods Overview

| Method | Description |
|--------|-------------|
| `get(id)` | Get a localized message by ID |
| `getWithArgs(id, args)` | Get a localized message with variable substitution |

---

## get(id)

Get a localized message by its identifier.

**Arguments:**
- `id: string` - The message ID to look up

**Returns:** `string` - The localized string, or `[id]` if not found

**Example:**
```javascript
const welcomeMessage = Locale.get("welcome");
console.log(welcomeMessage); // "Welcome to the game!"

const exitLabel = Locale.get("menu-exit");
console.log(exitLabel); // "Exit"
```

---

## getWithArgs(id, args)

Get a localized message with variable substitution (interpolation).

**Arguments:**
- `id: string` - The message ID to look up
- `args: object` - Key-value pairs for substitution

**Returns:** `string` - The localized string with variables replaced

**Example:**
```javascript
// In your .ftl file: player-score = { $player } scored { $score } points!
const message = Locale.getWithArgs("player-score", {
    player: "Alice",
    score: "100"
});
console.log(message); // "Alice scored 100 points!"

// In your .ftl file: items-count = You have { $count } items
const itemsMsg = Locale.getWithArgs("items-count", { count: "5" });
console.log(itemsMsg); // "You have 5 items"
```

---

## Mod Locale Structure

Mods can include their own translations by creating a `locale/` directory within the mod folder:

```
mods/
  my-mod/
    mod.json
    client/
      index.js
    locale/
      en-US/
        main.ftl
      it-IT/
        main.ftl
      de-DE/
        main.ftl
```

### Fluent (.ftl) File Format

Locale files use the [Fluent](https://projectfluent.org/) format (`.ftl` extension):

**`locale/en-US/main.ftl`:**
```ftl
# Simple messages
welcome = Welcome to My Mod!
menu-start = Start Game
menu-settings = Settings
menu-exit = Exit

# Messages with variables
player-greeting = Hello, { $name }!
score-display = Score: { $score }
items-count = You have { $count } items

# Multiline messages
game-intro =
    Welcome to the adventure!
    Press any key to begin.

# Selectors for pluralization
items-remaining = { $count ->
    [one] { $count } item remaining
   *[other] { $count } items remaining
}
```

**`locale/it-IT/main.ftl`:**
```ftl
welcome = Benvenuto nel mio Mod!
menu-start = Inizia Gioco
menu-settings = Impostazioni
menu-exit = Esci

player-greeting = Ciao, { $name }!
score-display = Punteggio: { $score }
items-count = Hai { $count } oggetti
```

---

## Lookup Order

When you call `Locale.get("message-id")`:

1. **Mod's current locale** - Checks `mods/your-mod/locale/{current-locale}/main.ftl`
2. **Mod's fallback locale** - If not found, checks `mods/your-mod/locale/en-US/main.ftl`
3. **Global application locale** - Falls back to the client's global locale files
4. **Not found** - Returns `[message-id]` if no translation exists

This allows mods to:
- Override global application strings
- Provide mod-specific translations
- Fall back gracefully to the application's default strings

---

## Usage Examples

### Basic Localization

```javascript
export function onBootstrap() {
    // Display localized UI text
    const title = Locale.get("game-title");
    const startButton = Locale.get("menu-start");
    const exitButton = Locale.get("menu-exit");

    console.log(`Title: ${title}`);
    console.log(`Start: ${startButton}`);
    console.log(`Exit: ${exitButton}`);
}
```

### Dynamic Messages with Variables

```javascript
function showPlayerScore(playerName, score) {
    const message = Locale.getWithArgs("player-score", {
        player: playerName,
        score: score.toString()
    });
    console.log(message);
}

showPlayerScore("Alice", 1500);
// Output (en-US): "Alice scored 1500 points!"
// Output (it-IT): "Alice ha segnato 1500 punti!"
```

### UI Labels

```javascript
async function createMenu(window) {
    const startBtn = await World.spawn({
        Node: { width: 200, height: 50 },
        Button: {},
        BackgroundColor: "#4a90d9",
        Text: { value: Locale.get("menu-start"), font_size: 18, color: "#ffffff" }
    }, window);

    const settingsBtn = await World.spawn({
        Node: { width: 200, height: 50 },
        Button: {},
        BackgroundColor: "#4a90d9",
        Text: { value: Locale.get("menu-settings"), font_size: 18, color: "#ffffff" }
    }, window);

    const exitBtn = await World.spawn({
        Node: { width: 200, height: 50 },
        Button: {},
        BackgroundColor: "#d94a4a",
        Text: { value: Locale.get("menu-exit"), font_size: 18, color: "#ffffff" }
    }, window);
}
```

### Status Messages

```javascript
function updateInventory(itemCount) {
    const statusText = Locale.getWithArgs("items-remaining", {
        count: itemCount.toString()
    });
    // Uses Fluent selectors for proper pluralization
    console.log(statusText);
    // "1 item remaining" or "5 items remaining"
}
```

---

## Best Practices

1. **Use descriptive message IDs**: Prefer `menu-start-game` over `btn1`
2. **Group related messages**: Use prefixes like `menu-`, `error-`, `dialog-`
3. **Always provide fallback locale**: Include `en-US` translations as the fallback
4. **Use variables for dynamic content**: Never concatenate strings manually
5. **Test all supported locales**: Ensure translations exist for all target languages

---

## Notes

- All locale files must be named `main.ftl` and placed in a locale-specific directory (e.g., `locale/en-US/main.ftl`)
- The locale identifier format follows BCP 47 (e.g., `en-US`, `it-IT`, `de-DE`, `ja-JP`)
- Unicode bidirectional characters are automatically stripped from output for proper display in terminals and logs
- If a mod doesn't have a `locale/` directory, it simply uses the global application locale

---

## See Also

- [Fluent Project](https://projectfluent.org/) - Fluent localization syntax documentation
- [System API](./system.md) - `System.getGameInfo()` for getting game context
