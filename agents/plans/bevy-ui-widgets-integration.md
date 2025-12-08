# Piano di Integrazione Bevy UI Widgets in Staminal

## Stato Attuale

### ECS Mode Implementato
Staminal ha già un sistema ECS funzionante che espone i componenti nativi Bevy ai mod JavaScript:
- **Node**: layout flexbox (width, height, padding, margin, flex_direction, justify_content, align_items, etc.)
- **BackgroundColor**: colori RGBA o hex
- **Text**: testo con font_size, color, font
- **BorderRadius**: angoli arrotondati
- **Transform**: posizione, rotazione, scala
- **Sprite**: rendering 2D
- **Visibility**: controllo visibilità
- **Button**: marker per interattività + eventi click
- **Interaction**: stato hover/pressed (auto-aggiunto con Button)
- **ImageNode**: immagini UI con modalità (Auto, Stretch, Tiled, Sliced, Contain, Cover)

### Pseudo-Componenti per Button
- `HoverBackgroundColor`, `PressedBackgroundColor`, `DisabledBackgroundColor`: cambio colore automatico
- `Disabled`: stato disabilitato
- `on_click`: callback diretta

---

## Bevy 0.17 UI Widgets (Sperimentale)

Bevy 0.17 introduce `bevy_ui_widgets` (feature `experimental_bevy_ui_widgets`):

### Widget Headless Disponibili
| Widget | Descrizione | Stato |
|--------|-------------|-------|
| **Button** | Emette eventi Activate al click | Stabile |
| **Slider** | Editing f32 con range | Stabile |
| **Scrollbar** | Scrolling contenuti | Stabile |
| **Checkbox** | Toggle on/off | Stabile |
| **RadioButton/RadioGroup** | Selezione esclusiva | Stabile |
| **TextInput** | Input testuale | In sviluppo (PR #20326) |

### Componenti di Supporto
- `InteractionDisabled`: disabilita interazione
- `Hovered`: stato hover
- `Checked`: stato checkbox/radio
- `Pressed`: stato pressed
- `ValueChange`: evento cambio valore
- `Activate`: evento attivazione

### Feathers (Sperimentale)
Feature `experimental_bevy_feathers` - widget stilizzati per editor/strumenti:
- Temi
- Accessibilità (screen reader)
- Virtual keyboard

---

## Piano di Implementazione

### Fase 1: Abilitare bevy_ui_widgets

**File**: `apps/stam_client/Cargo.toml`

```toml
bevy = { version = "0.17", default-features = false, features = [
    # ... features esistenti ...
    "experimental_bevy_ui_widgets",  # NUOVO
] }
```

### Fase 2: Nuovi Componenti Nativi

Estendere `NativeComponent` in `bevy.rs`:

```rust
enum NativeComponent {
    // ... esistenti ...
    // NUOVI widget headless
    Slider,
    Checkbox,
    RadioButton,
    RadioGroup,
    Scrollbar,
}
```

### Fase 3: API JavaScript per Ogni Widget

#### 3.1 Slider

```javascript
// Spawn slider (parentId come secondo argomento)
const slider = await World.spawn({
    Node: { width: 200, height: 30 },
    Slider: {
        value: 50,           // valore iniziale (f32)
        min: 0,              // minimo
        max: 100,            // massimo
        step: 1,             // step (opzionale)
        on_change: (event) => {
            console.log("Nuovo valore:", event.value);
        }
    },
    BackgroundColor: "#333333",   // track
    SliderFill: "#4a90d9",        // fill (pseudo-componente)
    SliderThumb: {                // thumb (pseudo-componente)
        size: 20,
        color: "#ffffff"
    }
}, container);  // parent diretto, non oggetto

// Leggere/scrivere valore
const val = await slider.get("Slider");
await slider.update("Slider", { value: 75 });
```

#### 3.2 Checkbox

```javascript
const checkbox = await World.spawn({
    Node: { width: 24, height: 24 },
    Checkbox: {
        checked: false,
        on_change: (event) => {
            console.log("Checked:", event.checked);
        }
    },
    BackgroundColor: "#333333",
    CheckedBackgroundColor: "#4a90d9",  // quando checked
    BorderRadius: 4
}, container);  // parent diretto

// Toggle stato
await checkbox.update("Checkbox", { checked: true });
```

#### 3.3 RadioGroup + RadioButton

```javascript
const group = await World.spawn({
    Node: { flex_direction: "column" },
    RadioGroup: {
        value: "option1",  // valore selezionato
        on_change: (event) => {
            console.log("Selezionato:", event.value);
        }
    }
}, container);  // parent diretto

// Radio buttons figli
await World.spawn({
    Node: { width: 24, height: 24 },
    RadioButton: { value: "option1" },
    BackgroundColor: "#333333"
}, group);

await World.spawn({
    Node: { width: 24, height: 24 },
    RadioButton: { value: "option2" },
    BackgroundColor: "#333333"
}, group);
```

#### 3.4 Scrollbar

```javascript
const scrollContainer = await World.spawn({
    Node: { width: 300, height: 400, overflow: "scroll" },
    Scrollbar: {
        direction: "vertical",  // o "horizontal"
        thumb_size: 50,         // dimensione thumb (opzionale)
        on_scroll: (event) => {
            console.log("Scroll:", event.position);
        }
    }
}, container);  // parent diretto
```

#### 3.5 TextInput (quando disponibile)

```javascript
const input = await World.spawn({
    Node: { width: 200, height: 40, padding: 10 },
    TextInput: {
        value: "",
        placeholder: "Inserisci testo...",
        max_length: 100,
        on_change: (event) => {
            console.log("Testo:", event.value);
        },
        on_submit: (event) => {
            console.log("Inviato:", event.value);
        }
    },
    BackgroundColor: "#1a1a2e",
    Text: { font_size: 16, color: "#ffffff" }
}, container);  // parent diretto
```

### Fase 4: Implementazione Rust

Per ogni widget:

1. **Parsing JSON → Bevy Component** in `bevy.rs`:
   ```rust
   pub fn json_to_slider(json: &Value) -> Result<bevy::ui::widget::Slider, String> {
       let value = json.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
       let min = json.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
       let max = json.get("max").and_then(|v| v.as_f64()).unwrap_or(100.0) as f32;
       Ok(bevy::ui::widget::Slider::new(value, min..=max))
   }
   ```

2. **Event Handling** (sistema Bevy):
   ```rust
   fn handle_slider_events(
       mut events: EventReader<ValueChange<Slider>>,
       entity_registry: Res<ScriptEntityRegistry>,
       event_tx: Res<EventSenderRes>,
   ) {
       for event in events.read() {
           if let Some(script_id) = entity_registry.get_script_id(event.target) {
               let _ = event_tx.0.blocking_send(GraphicEvent::WidgetValueChange {
                   entity_id: script_id,
                   widget_type: "Slider".to_string(),
                   value: serde_json::json!({ "value": event.new_value }),
               });
           }
       }
   }
   ```

3. **Callback Registry** per `on_change`:
   - Estendere `EntityEventCallbackRegistry` per supportare widget callbacks
   - Pattern simile a `on_click` per Button

### Fase 5: Aggiornare Documentazione

1. Aggiornare `docs/graphic/ecs.md`:
   - Sezione "Widget Components"
   - Esempi per ogni widget

2. Aggiornare `docs/mods/js/graphic/ecs.md`:
   - API JavaScript completa
   - Esempi interattivi

3. Creare `docs/graphic/widgets.md`:
   - Overview widget system
   - Styling guidelines
   - Best practices

---

## Ordine di Implementazione Consigliato

1. **Slider** - Più richiesto per UI di gioco (volume, brightness, etc.)
2. **Checkbox** - Semplice e utile per settings
3. **RadioGroup/RadioButton** - Selezione esclusiva
4. **Scrollbar** - Per liste lunghe
5. **TextInput** - Quando Bevy lo stabilizza

---

## Considerazioni

### Pro
- Widget headless = massima flessibilità di styling
- Accessibilità built-in
- Integrazione nativa con ECS esistente
- Eventi Bevy → callback JavaScript già funzionante

### Contro
- API sperimentale → possibili breaking changes in Bevy 0.18
- TextInput non ancora pronto
- Nessun tema di default (devi stilizzare tutto)

### Alternativa: Widget Custom
Se Bevy cambia troppo l'API, potremmo implementare widget custom sopra l'ECS esistente:
- Slider: Node + drag handling
- Checkbox: Button + stato checked
- Più stabile ma più lavoro

---

## Stima Effort

| Componente | Effort |
|------------|--------|
| Slider | Medio |
| Checkbox | Basso |
| RadioGroup | Medio |
| Scrollbar | Alto (layout) |
| TextInput | Alto (dipende da Bevy) |
| Documentazione | Basso |
| **Totale** | ~2-3 settimane |

---

## Prossimi Passi

1. Verificare che `experimental_bevy_ui_widgets` compili correttamente
2. Studiare l'API Bevy per ogni widget (esempi in `bevy/examples/ui/`)
3. Iniziare con Slider come proof-of-concept
4. Iterare su feedback

---

## Fonti

- [Bevy 0.17 Release Notes](https://bevy.org/news/bevy-0-17/)
- [bevy_ui_widgets Experimental Status](https://github.com/bevyengine/bevy/issues/20957)
- [TextInput PR](https://github.com/bevyengine/bevy/pull/20326)
