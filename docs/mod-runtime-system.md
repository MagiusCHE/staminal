# Mod Runtime System

## Overview

Il sistema di runtime modulare permette ai mod di utilizzare diversi linguaggi di scripting (JavaScript, Lua, C#, Rust, C++) in modo trasparente. Il client determina automaticamente quale runtime utilizzare in base all'estensione del file `entry_point` specificato nel manifest del mod.

## Architettura

### Componenti Principali

1. **`ModRuntimeManager`** - Gestisce tutti i runtime e dispatcha le chiamate al runtime appropriato
2. **`RuntimeAdapter` trait** - Interfaccia comune che tutti i runtime devono implementare
3. **Runtime specifici** - Adapter per ogni linguaggio (es. `JsRuntimeAdapter`)
4. **`RuntimeType` enum** - Identifica il tipo di runtime in base all'estensione del file

### File Structure

```
src/mod_runtime/
├── mod.rs              # ModRuntimeManager e RuntimeAdapter trait
├── runtime_type.rs     # RuntimeType enum e logica di detection
└── js_adapter.rs       # Adapter per JavaScript/QuickJS
```

## Come Funziona

### 1. Inizializzazione

Quando il client viene avviato e si connette a un game server:

```rust
// Crea il manager
let mut runtime_manager = ModRuntimeManager::new();

// Registra il runtime JavaScript (uno condiviso per tutti i mod JS)
let js_runtime = JsRuntime::new(runtime_config)?;
runtime_manager.register_js_runtime(JsRuntimeAdapter::new(js_runtime));

// In futuro:
// runtime_manager.register_lua_runtime(...);
// runtime_manager.register_csharp_runtime(...);
```

### 2. Caricamento Mod

Il runtime viene selezionato automaticamente in base all'estensione del file:

```rust
// Il manager determina automaticamente il runtime da entry_point
runtime_manager.load_mod("my-mod", Path::new("./mods/my-mod/main.js"))?;
// -> Usa JavaScript runtime

runtime_manager.load_mod("another-mod", Path::new("./mods/another-mod/init.lua"))?;
// -> Userebbe Lua runtime (quando implementato)
```

### 3. Chiamata Funzioni

Le chiamate alle funzioni dei mod sono completamente astratte:

```rust
// Il client non sa (e non deve sapere) quale runtime usa questo mod
runtime_manager.call_mod_function("my-mod", "onAttach")?;
runtime_manager.call_mod_function("my-mod", "onBootstrap")?;

// Con valori di ritorno
let result = runtime_manager.call_mod_function_with_return("my-mod", "getVersion")?;
match result {
    ModReturnValue::String(s) => println!("Version: {}", s),
    ModReturnValue::Int(i) => println!("Version: {}", i),
    ModReturnValue::Bool(b) => println!("Enabled: {}", b),
    ModReturnValue::None => println!("No return value"),
}
```

## Mapping Estensioni → Runtime

| Estensione | Runtime Type | Status |
|------------|-------------|---------|
| `.js` | JavaScript (QuickJS) | ✅ Implementato |
| `.lua` | Lua | 🔄 Futuro |
| `.cs` | C# (Mono/CoreCLR) | 🔄 Futuro |
| `.rs` | Rust (compiled) | 🔄 Futuro |
| `.cpp`, `.cc`, `.cxx` | C++ (compiled) | 🔄 Futuro |

## RuntimeAdapter Trait

Tutti i runtime devono implementare questo trait:

```rust
pub trait RuntimeAdapter {
    /// Carica un mod script in questo runtime
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str)
        -> Result<(), Box<dyn std::error::Error>>;

    /// Chiama una funzione in un mod senza valore di ritorno
    fn call_mod_function(&mut self, mod_id: &str, function_name: &str)
        -> Result<(), Box<dyn std::error::Error>>;

    /// Chiama una funzione in un mod con valore di ritorno
    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>>;
}
```

## Vantaggi dell'Architettura

### 1. **Un Runtime per Tipo**
Invece di creare un'istanza di runtime per ogni mod, si crea un'unica istanza per tipo di linguaggio:
- 5 mod JavaScript → 1 solo runtime JavaScript condiviso
- 3 mod Lua → 1 solo runtime Lua condiviso
- Risparmio di memoria e overhead

### 2. **Dispatch Dinamico**
Il client non ha bisogno di sapere quale runtime usa un mod:
```rust
// Funziona con qualsiasi tipo di mod!
runtime_manager.call_mod_function(mod_id, "onAttach")?;
```

### 3. **Estensibilità**
Aggiungere un nuovo runtime richiede solo:
1. Implementare `RuntimeAdapter` trait
2. Aggiungere l'estensione in `RuntimeType::from_extension()`
3. Registrare il runtime nel manager

### 4. **Type Safety**
I valori di ritorno sono type-safe grazie all'enum `ModReturnValue`:
```rust
pub enum ModReturnValue {
    None,
    String(String),
    Bool(bool),
    Int(i32),
}
```

## Esempio Completo

### Manifest del Mod (manifest.json)
```json
{
    "name": "My JavaScript Mod",
    "version": "1.0.0",
    "entry_point": "main.js",
    "priority": 100
}
```

### Codice Mod (main.js)
```javascript
function onAttach() {
    console.log("Mod attached!");
}

function onBootstrap() {
    console.log("Bootstrapping...");
    console.log("Data path:", process.app.data_path);
}

function getModInfo() {
    return "My Awesome Mod v1.0";
}
```

### Codice Client (Rust)
```rust
// Inizializza
let mut runtime_manager = ModRuntimeManager::new();
runtime_manager.register_js_runtime(js_adapter);

// Carica mod (automaticamente riconosce .js)
runtime_manager.load_mod("my-mod", Path::new("./mods/my-mod/main.js"))?;

// Chiama lifecycle hooks
runtime_manager.call_mod_function("my-mod", "onAttach")?;
runtime_manager.call_mod_function("my-mod", "onBootstrap")?;

// Ottieni informazioni
let info = runtime_manager.call_mod_function_with_return("my-mod", "getModInfo")?;
if let ModReturnValue::String(s) = info {
    println!("Mod info: {}", s);
}
```

## Implementazione Futuri Runtime

### Esempio: Aggiungere Lua

1. **Creare adapter** (`src/mod_runtime/lua_adapter.rs`):
```rust
pub struct LuaRuntimeAdapter {
    runtime: LuaRuntime,
}

impl RuntimeAdapter for LuaRuntimeAdapter {
    fn load_mod(&mut self, mod_path: &Path, mod_id: &str) -> Result<(), Box<dyn Error>> {
        // Carica script Lua
    }

    fn call_mod_function(&mut self, mod_id: &str, function_name: &str) -> Result<(), Box<dyn Error>> {
        // Chiama funzione Lua
    }

    // ...
}
```

2. **Aggiornare RuntimeType**:
```rust
match extension {
    "js" => Ok(RuntimeType::JavaScript),
    "lua" => Ok(RuntimeType::Lua),  // <-- Aggiungi qui
    // ...
}
```

3. **Registrare nel client**:
```rust
let lua_runtime = LuaRuntime::new()?;
runtime_manager.register_lua_runtime(LuaRuntimeAdapter::new(lua_runtime));
```

## Best Practices

1. **Condivisione Runtime**: Un runtime per tipo di linguaggio, non per mod
2. **Error Handling**: Tutti gli errori sono propagati con dettagli specifici del runtime
3. **Lifecycle Hooks**: Tutti i mod supportano `onAttach`, `onBootstrap`, ecc.
4. **Type Conversion**: I valori di ritorno sono convertiti in tipi Rust standard

## Limitazioni Attuali

1. Solo JavaScript è implementato
2. I valori di ritorno sono limitati a: None, String, Bool, Int
3. Non supporta ancora oggetti complessi o array (ma possibile via JSON)

## Roadmap

- [ ] Implementare runtime Lua
- [ ] Implementare runtime C#
- [ ] Supportare valori di ritorno complessi (oggetti, array)
- [ ] Aggiungere sandboxing per sicurezza
- [ ] Hot-reload dei mod senza riavviare il client
