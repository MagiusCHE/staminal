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

## Timer System (setTimeout/setInterval)

### Architettura Multi-Runtime Safe

Il sistema dei timer è progettato per funzionare correttamente con **multipli runtime simultanei** (JavaScript, Lua, C#, ecc.).

#### Componenti Chiave

1. **`NEXT_TIMER_ID`** (AtomicU32 globale)
   - Contatore atomico che garantisce ID unici **attraverso TUTTI i runtime**
   - Se JavaScript crea timer 1, 2, 3 → Lua otterrà 4, 5, 6 → C# otterrà 7, 8, 9
   - **Nessuna collisione possibile** tra runtime diversi

2. **`TIMER_ABORT_HANDLES`** (HashMap globale)
   - Registry condiviso: `timer_id -> Arc<Notify>`
   - Permette a `clearTimeout(id)` di funzionare indipendentemente da quale runtime ha creato il timer
   - Thread-safe tramite `Mutex`

#### Schema Architetturale

```
┌─────────────────────────────────────────────────────────────┐
│                     PROCESSO CLIENT                          │
├─────────────────────────────────────────────────────────────┤
│  NEXT_TIMER_ID (AtomicU32) ─────────────────────────────────│
│  TIMER_ABORT_HANDLES (Mutex<HashMap>) ──────────────────────│
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │ JsRuntime    │  │ LuaRuntime   │  │ CSharpRuntime│       │
│  │ (mod1.js)    │  │ (mod2.lua)   │  │ (mod3.cs)    │       │
│  │ timer: 1,2,3 │  │ timer: 4,5   │  │ timer: 6,7   │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### Implementazione JavaScript (rquickjs)

```rust
// In bindings.rs
static NEXT_TIMER_ID: AtomicU32 = AtomicU32::new(1);

static TIMER_ABORT_HANDLES: LazyLock<Mutex<HashMap<u32, Arc<Notify>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn set_timeout_interval<'js>(
    ctx: Ctx<'js>,
    cb: Function<'js>,
    msec: Option<u64>,
    is_interval: bool,
) -> rquickjs::Result<u32> {
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);
    let delay = msec.unwrap_or(0).max(4); // 4ms min per HTML5 spec

    let abort = Arc::new(Notify::new());
    TIMER_ABORT_HANDLES.lock().unwrap().insert(id, abort.clone());

    ctx.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = abort.notified() => break,
                _ = tokio::time::sleep(Duration::from_millis(delay)) => {
                    cb.call::<(), ()>(()).ok();
                    if !is_interval { break; }
                }
            }
        }
        TIMER_ABORT_HANDLES.lock().unwrap().remove(&id);
    });

    Ok(id)
}
```

#### Event Loop JavaScript

Per far funzionare i timer, il client deve eseguire il JS event loop:

```rust
// In main.rs
if let Some(js_runtime) = js_runtime_handle {
    tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => { /* shutdown */ }
        _ = maintain_game_connection(&mut stream, locale) => { /* connection closed */ }
        _ = run_js_event_loop(js_runtime) => { /* event loop exited */ }
    }
}
```

#### API Disponibili per i Mod

```javascript
// setTimeout - esegue callback dopo delay
const id = setTimeout(() => {
    console.log("Fired after 1000ms");
}, 1000);

// clearTimeout - cancella un timeout pendente
clearTimeout(id);

// setInterval - esegue callback ogni N ms
const intervalId = setInterval(() => {
    console.log("Tick!");
}, 500);

// clearInterval - cancella un interval
clearInterval(intervalId);
```

#### Note Implementative per Nuovi Runtime

Quando implementi timer per un nuovo runtime (Lua, C#, ecc.), usa le funzioni helper pubbliche esposte in `bindings.rs`:

```rust
// Funzioni pubbliche disponibili per tutti i runtime:
pub fn next_timer_id() -> u32;                                    // Genera ID unico
pub fn clear_timer(timer_id: u32);                                // Cancella timer
pub fn register_timer_abort_handle(timer_id: u32, abort: Arc<Notify>);  // Registra handle
pub fn remove_timer_abort_handle(timer_id: u32);                  // Rimuove handle
```

**Esempio per Lua adapter:**

```rust
use stam_mod_runtimes::adapters::js::bindings::{
    next_timer_id, register_timer_abort_handle, remove_timer_abort_handle, clear_timer
};
use tokio::sync::Notify;
use std::sync::Arc;

pub fn lua_set_timeout(delay_ms: u64, callback: LuaCallback) -> u32 {
    let id = next_timer_id();  // ID globalmente unico

    let abort = Arc::new(Notify::new());
    register_timer_abort_handle(id, abort.clone());

    tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = abort.notified() => { /* cancelled */ }
            _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => {
                callback.call();
            }
        }
        remove_timer_abort_handle(id);
    });

    id
}

pub fn lua_clear_timeout(timer_id: u32) {
    clear_timer(timer_id);  // Funziona anche per timer JS!
}
```

## Limitazioni Attuali

1. Solo JavaScript è implementato
2. I valori di ritorno sono limitati a: None, String, Bool, Int
3. Non supporta ancora oggetti complessi o array (ma possibile via JSON)

## Roadmap

- [x] Implementare setTimeout/setInterval per JavaScript
- [ ] Implementare runtime Lua
- [ ] Implementare runtime C#
- [ ] Supportare valori di ritorno complessi (oggetti, array)
- [ ] Aggiungere sandboxing per sicurezza
- [ ] Hot-reload dei mod senza riavviare il client
