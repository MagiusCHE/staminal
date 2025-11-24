# JavaScript Runtime - Return Values

## Overview

The JavaScript runtime supports calling functions with return values. This allows mods to provide data back to the Rust client.

## Available Methods

### `call_function_for_mod_string`
Calls a JavaScript function and returns a `String` value.

**Rust Example:**
```rust
match js_runtime.call_function_for_mod_string("getModName", "my-mod")? {
    Some(name) => println!("Mod name: {}", name),
    None => println!("Function not found (optional)"),
}
```

**JavaScript Example:**
```javascript
function getModName() {
    return "My Awesome Mod";
}
```

### `call_function_for_mod_bool`
Calls a JavaScript function and returns a `bool` value.

**Rust Example:**
```rust
match js_runtime.call_function_for_mod_bool("shouldEnableFeature", "my-mod")? {
    Some(enabled) => {
        if enabled {
            println!("Feature enabled");
        }
    }
    None => println!("Function not found (optional)"),
}
```

**JavaScript Example:**
```javascript
function shouldEnableFeature() {
    return true;
}
```

### `call_function_for_mod_int`
Calls a JavaScript function and returns an `i32` (integer) value.

**Rust Example:**
```rust
match js_runtime.call_function_for_mod_int("getVersion", "my-mod")? {
    Some(version) => println!("Version: {}", version),
    None => println!("Function not found (optional)"),
}
```

**JavaScript Example:**
```javascript
function getVersion() {
    return 42;
}
```

## Return Value Behavior

All `call_function_for_mod_*` methods return `Result<Option<T>, Box<dyn Error>>`:

- `Ok(Some(value))` - Function exists and returned a value successfully
- `Ok(None)` - Function doesn't exist (this is not an error - functions can be optional)
- `Err(...)` - JavaScript error occurred OR the return value cannot be converted to the expected type

## Type Conversion

QuickJS automatically handles type conversion between JavaScript and Rust:

| JavaScript Type | Rust Method | Notes |
|----------------|-------------|-------|
| `string` | `call_function_for_mod_string` | Any JS value can be converted to string |
| `boolean` | `call_function_for_mod_bool` | Truthy/falsy conversion applies |
| `number` | `call_function_for_mod_int` | Must be an integer value |

## Error Handling

If a JavaScript function throws an error, it will be formatted with:
- Error message
- Stack trace with file path and line numbers
- Function name and mod ID

Example error output:
```
Error: Invalid configuration
    at validateConfig (./workspace_data/demo/mods/my-mod/main.js:42:15)
    at onBootstrap (./workspace_data/demo/mods/my-mod/main.js:10:5)
```

## Best Practices

1. **Optional Functions**: Always handle the `None` case for optional functions:
   ```rust
   if let Some(result) = js_runtime.call_function_for_mod_string("optional", "mod")? {
       // Use result
   }
   ```

2. **Type Safety**: Choose the appropriate method for your expected return type. If conversion fails, an error will be returned.

3. **Error Context**: JavaScript errors will include the mod ID and function name for easier debugging.

## Future Extensions

For more complex return types (arrays, objects), you can:
1. Return JSON string and parse in Rust
2. Add new methods with custom type conversions using `FromJs` trait
3. Use multiple function calls to return multiple values

## Example: Configuration Loading

**JavaScript (main.js):**
```javascript
function getConfig() {
    return JSON.stringify({
        maxPlayers: 10,
        enablePvP: true,
        serverName: "My Server"
    });
}
```

**Rust:**
```rust
use serde_json::Value;

if let Some(config_json) = js_runtime.call_function_for_mod_string("getConfig", "my-mod")? {
    let config: Value = serde_json::from_str(&config_json)?;
    println!("Max players: {}", config["maxPlayers"]);
    println!("Enable PvP: {}", config["enablePvP"]);
}
```
