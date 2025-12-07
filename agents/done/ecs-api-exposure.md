# Piano: Ristrutturazione Comparto Graphic con Pattern ECS

## Obiettivo

Trasformare il sistema grafico di Staminal da un'API imperativa basata su widget predefiniti (Container, Text, Button, Image, Panel) a un'API che espone il pattern **ECS (Entity-Component-System)** di Bevy direttamente ai mod, permettendo:

1. Creazione di entity arbitrarie
2. Aggiunta/rimozione di component personalizzati
3. Query di entity e component
4. Registrazione di system custom dai mod

## Analisi Stato Attuale

### Architettura Corrente

```
JavaScript Mod (Worker Thread)
    ↓ (API imperativa: createWidget, setProperty)
GraphicProxy (stam_mod_runtimes/api/graphic/)
    ↓ (GraphicCommand via std::sync::mpsc)
BevyEngine Main Thread
    ↓ (GraphicEvent via tokio::sync::mpsc)
Event back to worker thread
```

**Limitazioni:**
- Widget predefiniti (5 tipi): Container, Text, Button, Image, Panel
- Config predefinita (WidgetConfig con ~50 campi)
- Non si possono creare entity custom
- Non si possono aggiungere component arbitrari
- Non si possono fare query ECS
- Non si possono registrare system

### Riferimenti Esterni

Da [bevy_mod_scripting](https://github.com/makspll/bevy_mod_scripting):
- Usa `ScriptQueryBuilder` per costruire query
- Supporta `with()` / `without()` per filtrare component
- `WorldGuard` per accesso sicuro al World
- Sistema di reflection per esporre component

---

## Proposta Architetturale

### Approccio: "Dual API"

Mantenere compatibilità con l'API esistente (widget-based) mentre si aggiunge un nuovo layer ECS:

```
┌─────────────────────────────────────────────────────────────┐
│                     MOD JAVASCRIPT                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────┐    ┌─────────────────────────────┐ │
│  │  API Widget (Alto)  │    │    API ECS (Basso Livello)  │ │
│  │                     │    │                              │ │
│  │  Graphic.createWin  │    │  World.spawn()              │ │
│  │  window.createWidget│    │  World.query()              │ │
│  │  widget.setContent  │    │  entity.insert(Component)   │ │
│  │                     │    │  entity.get(Component)      │ │
│  └─────────┬───────────┘    └──────────────┬──────────────┘ │
│            │                               │                 │
│            └───────────┬───────────────────┘                 │
│                        ▼                                     │
│             ┌─────────────────────┐                          │
│             │    GraphicProxy     │                          │
│             │   (Unified Layer)   │                          │
│             └──────────┬──────────┘                          │
└────────────────────────│────────────────────────────────────┘
                         │ Commands
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                    BEVY MAIN THREAD                          │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                   ECS World                           │   │
│  │                                                       │   │
│  │   Entities ←──→ Components ←──→ Systems              │   │
│  │                                                       │   │
│  │   - StamEntity (marker + script bindings)            │   │
│  │   - Reflect-able components only                     │   │
│  │   - Sandboxed access via commands                    │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

---

## Nuova API JavaScript Proposta

### 1. World API

```javascript
// Spawn entity vuota
const entity = await World.spawn();

// Spawn entity con component
const player = await World.spawn({
    Transform: { x: 100, y: 200 },
    Sprite: { path: "@my-mod/player.png" },
    Player: { health: 100, name: "Hero" }
});

// Despawn entity
await World.despawn(entity);
```

### 2. Entity API

```javascript
// Aggiungere component
await entity.insert("Velocity", { x: 5, y: 0 });
await entity.insert("Health", { current: 100, max: 100 });

// Rimuovere component
await entity.remove("Velocity");

// Leggere component
const transform = await entity.get("Transform");
console.log(transform.x, transform.y);

// Modificare component
await entity.set("Transform", { x: 150, y: 200 });

// Verificare presenza component
const hasVelocity = await entity.has("Velocity");
```

### 3. Query API

```javascript
// Query semplice
const enemies = await World.query()
    .with("Enemy")
    .with("Transform")
    .without("Dead")
    .build();

for (const result of enemies) {
    const entity = result.entity;
    const transform = result.get("Transform");
    const enemy = result.get("Enemy");

    // Update
    await entity.set("Transform", {
        x: transform.x + enemy.speed,
        y: transform.y
    });
}

// Query con filtri
const nearbyEntities = await World.query()
    .with("Transform")
    .filter((e) => {
        const t = e.get("Transform");
        return Math.abs(t.x - playerX) < 100;
    })
    .build();
```

### 4. Component Registration

```javascript
// Registrare component custom (in onAttach)
World.registerComponent("Player", {
    health: "number",
    name: "string",
    inventory: "array"
});

World.registerComponent("Velocity", {
    x: "number",
    y: "number"
});

// I component devono essere registrati prima dell'uso
```

### 5. System Declaration (Approccio Ibrido)

#### Il Problema dei Custom Systems

Eseguire callback JavaScript ogni frame per ogni entity è **non praticabile** con l'architettura a messaggi:

```
Frame N (16ms budget @ 60fps)
│
│  Con 100 entity, un system JS dovrebbe:
│  - 100x await entity.get("Transform")  → round-trip channel
│  - 100x await entity.get("Velocity")   → round-trip channel
│  - 100x await entity.set("Transform")  → round-trip channel
│
│  = 300 round-trip × ~100μs = 30ms (già oltre il budget!)
```

#### Soluzione: Dichiarazione + Esecuzione Rust-side

I mod **dichiarano** il comportamento, Bevy lo **esegue** nativamente. Tre livelli di astrazione:

##### Livello 1: Behaviors Predefiniti (Massima Performance)

```javascript
// Comportamenti comuni già implementati in Rust
World.declareSystem("PlayerMovement", {
    query: { with: ["Transform", "Velocity"], without: ["Frozen"] },
    behavior: "apply_velocity"  // Nome di un system Rust predefinito
});

World.declareSystem("GravitySystem", {
    query: { with: ["Velocity", "AffectedByGravity"] },
    behavior: "apply_gravity",
    config: { strength: 9.8, direction: "down" }
});

World.declareSystem("HealthRegen", {
    query: { with: ["Health", "Regeneration"] },
    behavior: "regenerate_over_time",
    config: { field: "current", rate: 5, max_field: "max", interval: 1.0 }
});

World.declareSystem("FollowTarget", {
    query: { with: ["Transform", "FollowBehavior"] },
    behavior: "follow_entity",
    config: { speed_field: "speed", target_field: "target_entity" }
});
```

Behaviors disponibili (implementati in Rust):
- `apply_velocity` - Aggiunge Velocity a Transform
- `apply_gravity` - Applica gravità a Velocity
- `apply_friction` - Riduce Velocity nel tempo
- `regenerate_over_time` - Incrementa un campo numerico
- `decay_over_time` - Decrementa un campo numerico
- `follow_entity` - Muove verso un'altra entity
- `orbit_around` - Orbita attorno a un punto/entity
- `bounce_on_bounds` - Rimbalza ai bordi
- `despawn_when_zero` - Despawna quando un campo è 0
- `animate_sprite` - Cicla frame di animazione

##### Livello 2: Expression Formulas (Flessibilità Matematica)

Per casi non coperti dai behaviors, formule matematiche parsate e compilate:

```javascript
World.declareSystem("Oscillate", {
    query: { with: ["Transform", "Oscillator"] },
    formulas: [
        "Transform.x = Oscillator.center_x + sin(time * Oscillator.speed) * Oscillator.amplitude",
        "Transform.y = Oscillator.center_y + cos(time * Oscillator.speed) * Oscillator.amplitude"
    ]
});

World.declareSystem("ScaleByHealth", {
    query: { with: ["Transform", "Health"] },
    formulas: [
        "Transform.scale = 0.5 + (Health.current / Health.max) * 0.5"
    ]
});

World.declareSystem("FadeOut", {
    query: { with: ["Sprite", "Lifetime"] },
    formulas: [
        "Sprite.alpha = Lifetime.remaining / Lifetime.total"
    ]
});
```

Variabili disponibili nelle formule:
- `dt` - Delta time (secondi dal frame precedente)
- `time` - Tempo totale dall'avvio (secondi)
- `ComponentName.field` - Accesso a campi dei component nella query
- Funzioni math: `sin`, `cos`, `tan`, `abs`, `min`, `max`, `clamp`, `lerp`, `sqrt`, `pow`

Rust compila le formule in AST al momento della dichiarazione e le esegue nativamente ogni frame.

##### Livello 3: Event-Driven Logic (Logica Complessa)

Per logica che richiede condizioni, branching, o operazioni asincrone:

```javascript
// NON usare systems, usare eventi
System.registerEvent("game:tick", async (req, res) => {
    // Eseguito quando il mod decide, non ogni frame
    const lowHealthEnemies = await World.query()
        .with("Enemy", "Health")
        .build();

    for (const result of lowHealthEnemies) {
        const health = result.get("Health");
        if (health.current < health.max * 0.2) {
            // Logica complessa: fuga, richiesta rinforzi, etc.
            await result.entity.insert("Fleeing", { target: "spawn_point" });
            await result.entity.remove("Aggressive");
        }
    }
});

// Evento collision gestito dal mod
System.registerEvent("physics:collision", async (req, res) => {
    const { entityA, entityB } = req;

    const aIsPlayer = await entityA.has("Player");
    const bIsEnemy = await entityB.has("Enemy");

    if (aIsPlayer && bIsEnemy) {
        const playerHealth = await entityA.get("Health");
        const enemyDamage = await entityB.get("Enemy");

        await entityA.set("Health", {
            ...playerHealth,
            current: playerHealth.current - enemyDamage.damage
        });

        // Spawn effetto danno
        await World.spawn({
            Transform: await entityA.get("Transform"),
            ParticleEffect: { type: "damage_flash", duration: 0.2 }
        });
    }
});
```

#### Quando Usare Cosa

| Caso d'uso | Approccio | Esempio |
|------------|-----------|---------|
| Movimento base | Behavior predefinito | `apply_velocity` |
| Fisica semplice | Behavior predefinito | `apply_gravity`, `apply_friction` |
| Animazioni parametriche | Formula | `sin(time * speed) * amplitude` |
| Scaling/fading | Formula | `alpha = remaining / total` |
| AI con decisioni | Event-driven | Collision handlers, tick events |
| Spawn/despawn condizionale | Event-driven | `if (health <= 0) despawn()` |
| Interazioni player | Event-driven | Input handling, dialoghi |

---

## Implementazione Tecnica

### Fase 1: Infrastruttura Base

#### 1.1 Nuovi Comandi ECS

```rust
// In commands.rs - nuovi comandi
pub enum GraphicCommand {
    // ... esistenti ...

    // === ECS Commands ===

    /// Spawn a new entity
    SpawnEntity {
        /// Components to add (JSON-serialized)
        components: HashMap<String, serde_json::Value>,
        response_tx: oneshot::Sender<Result<u64, String>>,
    },

    /// Despawn an entity
    DespawnEntity {
        entity_id: u64,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Insert component on entity
    InsertComponent {
        entity_id: u64,
        component_name: String,
        component_data: serde_json::Value,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Remove component from entity
    RemoveComponent {
        entity_id: u64,
        component_name: String,
        response_tx: oneshot::Sender<Result<(), String>>,
    },

    /// Get component data
    GetComponent {
        entity_id: u64,
        component_name: String,
        response_tx: oneshot::Sender<Result<serde_json::Value, String>>,
    },

    /// Query entities
    QueryEntities {
        with_components: Vec<String>,
        without_components: Vec<String>,
        response_tx: oneshot::Sender<Result<Vec<QueryResult>, String>>,
    },

    /// Register a custom component type
    RegisterComponent {
        name: String,
        schema: ComponentSchema,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
}
```

#### 1.2 Component Registry (Rust)

```rust
// Nuovo file: apps/shared/stam_mod_runtimes/src/api/graphic/ecs/mod.rs

use bevy::reflect::TypeRegistry;
use serde_json::Value;

/// Schema per component custom definiti dai mod
#[derive(Clone, Debug)]
pub struct ComponentSchema {
    pub name: String,
    pub fields: HashMap<String, FieldType>,
}

#[derive(Clone, Debug)]
pub enum FieldType {
    Number,
    String,
    Bool,
    Vec2,
    Vec3,
    Color,
    Array(Box<FieldType>),
    Object(HashMap<String, FieldType>),
}

/// Registry per component custom (non-Bevy)
pub struct ScriptComponentRegistry {
    schemas: HashMap<String, ComponentSchema>,
}

impl ScriptComponentRegistry {
    pub fn register(&mut self, schema: ComponentSchema) -> Result<(), String>;
    pub fn validate(&self, name: &str, data: &Value) -> Result<(), String>;
    pub fn get_schema(&self, name: &str) -> Option<&ComponentSchema>;
}
```

#### 1.3 Entity Registry (Bevy side)

```rust
// In bevy.rs - nuovo registry

/// Marker component per entity create via script
#[derive(Component)]
pub struct ScriptEntity {
    /// ID esposto agli script
    pub script_id: u64,
    /// Mod che ha creato questa entity
    pub owner_mod: String,
}

/// Component per dati custom definiti da script
#[derive(Component, Reflect)]
pub struct ScriptComponent {
    /// Nome del component type
    pub type_name: String,
    /// Dati serializzati
    pub data: serde_json::Value,
}

/// Registry per mappare script_id <-> Bevy Entity
#[derive(Resource)]
pub struct ScriptEntityRegistry {
    id_to_entity: HashMap<u64, Entity>,
    entity_to_id: HashMap<Entity, u64>,
    next_id: AtomicU64,
}
```

### Fase 2: Integrazione Bevy Reflect

Per component Bevy nativi (Transform, Sprite, ecc.), usare il sistema Reflect:

```rust
// Accesso a component Bevy tramite reflection
fn get_component_reflected(
    world: &World,
    entity: Entity,
    type_registry: &TypeRegistry,
    component_name: &str,
) -> Result<serde_json::Value, String> {
    // 1. Trova il TypeId dal nome
    // 2. Usa reflection per leggere il component
    // 3. Serializza a JSON
}
```

### Fase 3: Query System

```rust
/// Risultato di una query
#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub entity_id: u64,
    pub components: HashMap<String, serde_json::Value>,
}

/// Esegue una query ECS
fn execute_query(
    world: &World,
    with_components: &[String],
    without_components: &[String],
    script_registry: &ScriptComponentRegistry,
    entity_registry: &ScriptEntityRegistry,
) -> Vec<QueryResult> {
    // Per ogni entity con ScriptEntity marker:
    // 1. Verifica che abbia tutti i component "with"
    // 2. Verifica che NON abbia i component "without"
    // 3. Raccogli i component richiesti
    // 4. Ritorna QueryResult
}
```

---

## Sfide e Soluzioni

### 1. Thread Safety

**Problema:** Bevy World non è thread-safe, ma i mod girano su worker thread.

**Soluzione:** Manteniamo l'architettura a messaggi esistente. Tutte le operazioni ECS passano attraverso il command channel e vengono eseguite nel main thread Bevy.

### 2. Performance Query

**Problema:** Query frequenti potrebbero essere lente con message passing.

**Soluzione:**
- Cache locale nel proxy per dati read-only
- Batch di operazioni in un singolo command
- Sistema di subscription per notifiche push invece di polling

### 3. Component Custom vs Bevy Native

**Problema:** Come gestire sia component custom (definiti da mod) che component Bevy nativi (Transform, Sprite)?

**Soluzione:**
- Component custom: stored come `ScriptComponent` con JSON data
- Component Bevy: accesso via Reflect system
- API unificata che nasconde la differenza

### 4. Sicurezza

**Problema:** I mod non devono poter accedere a entity/component di sistema.

**Soluzione:**
- Marker `ScriptEntity` per entity create da script
- Whitelist di component Bevy accessibili
- Sandboxing: mod possono solo accedere a proprie entity

---

## Piano di Implementazione

### Milestone 1: Spawn/Despawn Base (1-2 giorni)
- [ ] Aggiungere comandi `SpawnEntity`, `DespawnEntity`
- [ ] Creare `ScriptEntityRegistry` in Bevy
- [ ] Binding JS per `World.spawn()` / `World.despawn()`
- [ ] Test base

### Milestone 2: Component Operations (2-3 giorni)
- [ ] Comandi `InsertComponent`, `RemoveComponent`, `GetComponent`
- [ ] `ScriptComponent` per dati custom
- [ ] `ScriptComponentRegistry` per validazione
- [ ] Binding JS per `entity.insert/remove/get/set`
- [ ] Test con component custom

### Milestone 3: Query System (2-3 giorni)
- [ ] Comando `QueryEntities`
- [ ] Implementazione query nel main thread
- [ ] Binding JS per `World.query().with().without().build()`
- [ ] Test query

### Milestone 4: Bevy Reflect Integration (3-4 giorni)
- [ ] Mapping component Bevy nativi via Reflect
- [ ] Whitelist component sicuri (Transform, Sprite, etc.)
- [ ] Serializzazione/deserializzazione con Reflect
- [ ] Test con Transform, Sprite

### Milestone 5: Widget API su ECS (2-3 giorni)
- [ ] Reimplementare widget esistenti come entity+components
- [ ] Backward compatibility con API widget esistente
- [ ] Documentazione migrazione

### Milestone 6: Declared Systems - Behaviors (2-3 giorni)
- [ ] Definire enum `SystemBehavior` con comportamenti predefiniti
- [ ] Comando `DeclareSystem` con behavior + config
- [ ] Implementare behaviors: `apply_velocity`, `apply_gravity`, `apply_friction`
- [ ] Sistema Bevy che esegue i declared systems ogni frame
- [ ] Binding JS per `World.declareSystem()` con behavior
- [ ] Test behaviors base

### Milestone 7: Declared Systems - Formulas (3-4 giorni)
- [ ] Integrare crate `evalexpr` o `fasteval` per parsing formule
- [ ] Compilazione formula → AST al momento della dichiarazione
- [ ] Variabili context: `dt`, `time`, campi component
- [ ] Sistema Bevy che valuta formule ogni frame
- [ ] Binding JS per `World.declareSystem()` con formulas
- [ ] Test formule matematiche

### Milestone 8 (Opzionale): Behaviors Avanzati
- [ ] `follow_entity`, `orbit_around`
- [ ] `bounce_on_bounds`, `despawn_when_zero`
- [ ] `animate_sprite`
- [ ] Documentazione behaviors disponibili

---

## Backward Compatibility

L'API widget esistente rimane funzionante. Internamente, viene reimplementata usando l'API ECS:

```javascript
// Vecchia API (continua a funzionare)
const button = await window.createWidget(WidgetTypes.Button, {
    label: "Click me",
    backgroundColor: "#4a90d9"
});

// Equivalente nuova API ECS
const button = await World.spawn({
    UiNode: { width: "auto", height: "auto" },
    UiButton: { label: "Click me" },
    BackgroundColor: { color: "#4a90d9" },
    Interaction: {},
    TargetCamera: { window_id: window.id }
});
```

---

## Rischi e Mitigazioni

| Rischio | Probabilità | Impatto | Mitigazione |
|---------|-------------|---------|-------------|
| Performance degradata per message passing | Media | Alto | Batch operations, caching, profiling |
| Complessità API per mod developer | Alta | Medio | Mantenere API widget come layer alto livello |
| Breaking changes Bevy | Bassa | Alto | Versionare API, astrarre component Bevy |
| Sicurezza sandbox | Media | Alto | Whitelist rigida, marker entity |

---

## Conclusione

Sì, è possibile esporre il pattern ECS di Bevy ai mod. L'approccio raccomandato è:

1. **Dual API**: Mantenere l'API widget ad alto livello per semplicità, aggiungere API ECS per flessibilità
2. **Message-based**: Continuare a usare channels per thread safety
3. **Incremental**: Implementare in milestone, testare ogni fase
4. **Secure**: Sandbox con whitelist e marker entity
5. **Systems Ibridi**: Tre livelli di astrazione per bilanciare performance e flessibilità:
   - **Behaviors predefiniti** (Rust-native, massima performance)
   - **Expression formulas** (parsate e compilate, flessibilità matematica)
   - **Event-driven** (JS async, logica complessa on-demand)

Il lavoro stimato è di **3-4 settimane** per le funzionalità core (Milestone 1-7), con behaviors avanzati come estensione opzionale.
