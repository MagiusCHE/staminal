# Piano di Integrazione JavaScript Runtime nel Client Staminal

## 📋 Analisi Situazione Attuale

**Stato corrente:**
- ✅ Sistema di validazione mod (esistenza + versione) completato
- ✅ Manifest.json parsing implementato
- ✅ Bootstrap mods validati prima di continuare
- ❌ Nessun runtime JavaScript integrato
- 📄 Mod `mods-manager` ha `main.js` con funzione `onAttach()`

**Struttura mod esistente:**
```javascript
// main.js
function onAttach() {
    console.log("SimpleGUI mod attached.");
}
```

---

## 🎯 Obiettivi

1. Integrare un runtime JavaScript leggero nel client Rust
2. Caricare ed eseguire i bootstrap mods JavaScript
3. Fornire API bridge Rust ↔ JavaScript per:
   - Console logging
   - Accesso a funzionalità del client
   - Event system (onAttach, onDetach, etc.)

---

## 🔍 Scelta del Runtime JavaScript

**Opzioni valutate:**

| Runtime | Pro | Contro | Peso |
|---------|-----|--------|------|
| **QuickJS** (via `rquickjs`) | Leggero (~600KB), veloce bootstrap, ottima integrazione Rust | Meno features ES6+ avanzate | ⭐⭐⭐⭐⭐ |
| **V8** (via `rusty_v8` o `deno_core`) | Standard completo, performance eccellenti | Pesante (~20MB), compile time lungo | ⭐⭐⭐ |
| **Boa** | Scritto in Rust puro, facile debug | Meno maturo, performance inferiori | ⭐⭐ |

**✅ RACCOMANDAZIONE: QuickJS (crate `rquickjs`)**

Motivi:
- Footprint minimo (ideale per client gaming)
- Ottima integrazione con Rust attraverso `rquickjs`
- Sufficiente per UI mods e scripting
- Compile time ragionevole
- Supporto ES6+ base (classes, arrow functions, promises)

---

## 📐 Architettura Proposta

```
┌─────────────────────────────────────────────────────────┐
│                   STAMINAL CLIENT                       │
├─────────────────────────────────────────────────────────┤
│  1. Bootstrap Validation (✅ già implementato)          │
│     - Verifica esistenza mod                            │
│     - Verifica versione                                 │
├─────────────────────────────────────────────────────────┤
│  2. JavaScript Runtime Manager (🆕 da implementare)     │
│     ┌───────────────────────────────────────────┐       │
│     │ ModRuntime                                │       │
│     │  - QuickJS Context                        │       │
│     │  - Loaded Modules Map                     │       │
│     │  - API Bindings (Console, Client, etc.)  │       │
│     └───────────────────────────────────────────┘       │
├─────────────────────────────────────────────────────────┤
│  3. Mod Lifecycle (🆕 da implementare)                  │
│     - load_bootstrap_mods()                             │
│     - execute_entry_point("main.js")                    │
│     - call_function("onAttach")                         │
│     - (futuro) call_function("onDetach")                │
├─────────────────────────────────────────────────────────┤
│  4. API Bridge Rust ↔ JS (🆕 da implementare)          │
│     - console.log/error/warn/info                       │
│     - client.send(message)                              │
│     - client.on(event, callback)                        │
│     - (futuro) ui.*, game.*, etc.                       │
└─────────────────────────────────────────────────────────┘
```

---

## 📝 Piano di Implementazione (Step by Step)

### **FASE 1: Setup Runtime Base** ⏱️ ~30min

1. **Aggiungere dipendenza `rquickjs`** a `apps/stam_client/Cargo.toml`:
   ```toml
   rquickjs = { version = "0.6", features = ["classes", "properties"] }
   ```

2. **Creare modulo `mod_runtime.rs`** in `apps/stam_client/src/`:
   - Struct `ModRuntime` con QuickJS context
   - Metodo `new()` per inizializzare runtime
   - Metodo `load_module(path, entry_point)` per caricare JS
   - Metodo `call_function(fn_name)` per invocare funzioni

3. **Implementare basic console API**:
   - Registrare `console.log()`, `console.error()`, etc.
   - Bridge verso tracing log del client

### **FASE 2: Caricamento Mod** ⏱️ ~20min

4. **Integrare nel flusso di bootstrap validation** (`main.rs`):
   ```rust
   // Dopo validazione versioni bootstrap mods
   let mut runtime = ModRuntime::new()?;

   for mod_info in &bootstrap_mods {
       let mod_dir = mods_dir.join(&mod_info.mod_id);
       let manifest = read_manifest(&mod_dir)?;

       // Carica ed esegui entry_point
       runtime.load_module(
           &mod_dir.join(&manifest.entry_point),
           &mod_info.mod_id
       )?;
   }
   ```

5. **Chiamare lifecycle hooks**:
   - Dopo caricamento: `runtime.call_function("onAttach")?`
   - Prima di disconnect (futuro): `runtime.call_function("onDetach")?`

### **FASE 3: API Bridge Avanzata** ⏱️ ~40min (opzionale, fase 2+)

6. **Esporre API client** (esempio):
   ```javascript
   // Disponibile nei mods
   client.send({ type: "chat", message: "Hello" });
   client.on("message", (msg) => console.log(msg));
   ```

7. **Event system**:
   - Registrare callback JS per eventi Rust
   - Dispatcher eventi server → mod JavaScript

---

## 🔧 Struttura File Proposta

```
apps/stam_client/src/
├── main.rs                  (✅ esistente - integrare chiamate runtime)
├── locale.rs                (✅ esistente)
├── mod_runtime.rs           (🆕 nuovo - ModRuntime struct)
└── mod_api/                 (🆕 nuovo - API bindings)
    ├── mod.rs
    ├── console.rs           (console.log, etc.)
    ├── client.rs            (client.send, client.on)
    └── events.rs            (event dispatcher)
```

---

## ⚠️ Considerazioni Importanti

**Sicurezza:**
- ⚠️ JavaScript mods possono eseguire codice arbitrario
- 🔒 Considerare sandbox (limitare access filesystem, network)
- 📋 Per ora: trust model (mods da source fidato)
- 🔮 Futuro: permission system nel manifest

**Performance:**
- QuickJS è single-threaded
- Esecuzione mod in async task separato (evitare block main thread)
- Timeout per prevent infinite loops

**Error Handling:**
- Catturare exceptions JavaScript
- Logging chiaro errori mod (quale mod, quale funzione)
- Fallback graceful se mod fallisce

---

## 🎬 Prossimi Passi Immediati

**Scelta 1: Implementazione Minima (Consigliata per MVP)**
1. Setup `rquickjs` dependency
2. Creare `ModRuntime` base con console.log
3. Caricare e eseguire `onAttach()` per bootstrap mods
4. Test con `mods-manager`

**Scelta 2: Implementazione Completa**
- Include tutto Fase 1-3
- API bridge completa
- Event system

**Scelta 3: Solo Piano (attendi feedback)**
- Fermarsi qui e discutere architettura

---

## 💬 Domande per Decisioni Implementative

1. **Quali feature API vuoi esporre subito ai mods?** (solo console.log o anche client.send, events, etc.?)

2. **Livello di sandboxing desiderato?** (accesso filesystem illimitato o limitato a mod directory?)

3. **Gestione errori mod:** se un bootstrap mod fallisce il load, bloccare avvio client o continuare?

4. **Preferisci implementazione incrementale** (prima console.log, poi API avanzate) **o tutto insieme?**

---

## 📚 Riferimenti

- **rquickjs documentation**: https://docs.rs/rquickjs/latest/rquickjs/
- **QuickJS official**: https://bellard.org/quickjs/
- **Staminal concept doc**: `docs/concept.md` (vedi sezione mod system)
