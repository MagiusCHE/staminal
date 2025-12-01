# Piano di Implementazione: Widget UI per GraphicEngine

## Sommario Esecutivo

Questo documento descrive l'implementazione del sistema di widget UI per Staminal, permettendo agli script dei mod di creare, modificare e gestire widget grafici all'interno delle finestre del GraphicEngine Bevy.

**Principio chiave**: Il sistema è **language-agnostic**. L'API core è definita in Rust e ogni runtime (JavaScript, Lua, C#, Rust, C++) implementa i propri binding verso questa API comune.

## Indice

1. [Analisi Architettura Attuale](#1-analisi-architettura-attuale)
2. [Scelta della Strategia UI](#2-scelta-della-strategia-ui)
3. [Design del Sistema Widget](#3-design-del-sistema-widget)
4. [API Core (Language-Agnostic)](#4-api-core-language-agnostic)
5. [Binding per Runtime Specifici](#5-binding-per-runtime-specifici)
6. [Implementazione Rust Core](#6-implementazione-rust-core)
7. [Sistema di Eventi e Callback](#7-sistema-di-eventi-e-callback)
8. [Piano di Implementazione](#8-piano-di-implementazione)

---

## 1. Analisi Architettura Attuale

### 1.1 Threading Model

```
┌─────────────────────────────────────────────────────┐
│              Main Thread (main.rs)                  │
│  ┌──────────────────────────────────────────────┐  │
│  │ BevyEngine                                    │  │
│  │  • Window management                         │  │
│  │  • Rendering pipeline                        │  │
│  │  • UI rendering (bevy_ui)                    │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
           ↕ channels (mpsc)
┌─────────────────────────────────────────────────────┐
│            Worker Thread (tokio runtime)            │
│  ┌──────────────────────────────────────────────┐  │
│  │ GraphicProxy (Language-Agnostic Core API)    │  │
│  │  • Sends commands to Bevy                    │  │
│  │  • Receives events from Bevy                 │  │
│  │  • Shared by ALL runtime adapters            │  │
│  └──────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────┐  │
│  │ Runtime Adapters (one per language)          │  │
│  │  ├── JavaScript (QuickJS) ← attuale          │  │
│  │  ├── Lua (mlua) ← futuro                     │  │
│  │  ├── C# (dotnet) ← futuro                    │  │
│  │  ├── Rust (native) ← futuro                  │  │
│  │  └── C++ (FFI) ← futuro                      │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 1.2 Flusso Comandi Esistente

1. **Script (qualsiasi linguaggio)** chiama l'API widget (es. `window.createWidget()`)
2. **Runtime Adapter** traduce la chiamata verso `GraphicProxy`
3. **GraphicProxy** genera un ID, invia `GraphicCommand::CreateWidget`
4. **Bevy** riceve il comando, crea l'entità widget, risponde
5. **GraphicProxy** memorizza `WidgetInfo`, restituisce ID al Runtime Adapter
6. **Runtime Adapter** crea l'oggetto Widget nel linguaggio specifico

### 1.3 File Chiave

| File | Descrizione |
|------|-------------|
| `apps/stam_client/src/engines/bevy.rs` | Implementazione BevyEngine |
| `apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs` | GraphicProxy |
| `apps/shared/stam_mod_runtimes/src/api/graphic/commands.rs` | Comandi |
| `apps/shared/stam_mod_runtimes/src/api/graphic/events.rs` | Eventi |
| `apps/shared/stam_mod_runtimes/src/adapters/js/bindings.rs` | Binding JS |

---

## 2. Scelta della Strategia UI

### 2.1 Opzioni Analizzate

| Opzione | Pro | Contro |
|---------|-----|--------|
| **bevy_ui nativo** | Integrazione perfetta ECS, nessuna dipendenza extra, futuro di Bevy | Verboso, API in evoluzione |
| **bevy_egui** | API immediate mode, facile da usare, documentazione | Dipendenza extra, stile diverso da Bevy |
| **Sickle UI** | Ergonomico, riduce boilerplate | Dipendenza extra, meno maturo |

### 2.2 Decisione: bevy_ui Nativo

**Motivazione:**
1. **Compatibilità Bevy**: Segue la direzione ufficiale del motore
2. **ECS Integration**: I widget sono entità, query native
3. **No dipendenze extra**: Riduce complessità e conflitti versioni
4. **Future-proof**: Bevy sta attivamente migliorando il sistema UI

**Trade-off accettato:**
- Maggiore verbosità lato Rust (non visibile agli script)
- Necessità di costruire astrazione per gli script

---

## 3. Design del Sistema Widget

### 3.1 Gerarchia Widget

```
Window (Bevy Entity)
 └── RootNode (Node, TargetCamera)
      └── Container (Node, Layout)
           ├── Text (Node, Text, TextColor)
           ├── Button (Node, Button, BackgroundColor)
           │    └── ButtonLabel (Text)
           ├── Image (Node, UiImage)
           └── Panel (Node, BackgroundColor)
                └── ... (nested widgets)
```

### 3.2 Widget Supportati (Fase 1)

| Widget | Componenti Bevy | Descrizione |
|--------|-----------------|-------------|
| `Container` | `Node` | Layout flexbox/grid |
| `Text` | `Node`, `Text`, `TextColor` | Testo statico o dinamico |
| `Button` | `Node`, `Button`, `BackgroundColor`, `BorderColor` | Pulsante cliccabile |
| `Image` | `Node`, `UiImage` | Immagine da asset |
| `Panel` | `Node`, `BackgroundColor` | Contenitore con sfondo |

### 3.3 Sistema di ID Widget

```rust
// Ogni widget ha un ID univoco generato da Staminal
pub struct WidgetId(u64);

// Registry in BevyEngine
pub struct WidgetRegistry {
    widgets: HashMap<u64, Entity>,
    next_id: AtomicU64,
}
```

### 3.4 Componente Marker

```rust
/// Marca un'entità come widget Staminal
#[derive(Component)]
pub struct StamWidget {
    pub id: u64,
    pub window_id: u64,
    pub widget_type: WidgetType,
}

#[derive(Clone, Copy, PartialEq)]
pub enum WidgetType {
    Container,
    Text,
    Button,
    Image,
    Panel,
}
```

---

## 4. API Core (Language-Agnostic)

L'API core è definita in Rust nel modulo `stam_mod_runtimes::api::graphic`. Ogni runtime adapter traduce queste strutture nel proprio linguaggio.

### 4.1 Principi di Design

1. **Strutture dati semplici**: Solo tipi primitivi e struct serializzabili
2. **ID-based references**: Widget referenziati tramite `u64` ID, non puntatori
3. **Async by default**: Tutte le operazioni che comunicano con Bevy sono async
4. **Eventi via channel**: Callback implementate come eventi, non come function pointers
5. **No language-specific types**: Nessun `Function`, `Closure`, o tipo specifico del linguaggio

### 4.2 Tipi Core Rust

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/widget.rs

/// Tipi di widget supportati
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WidgetType {
    Container,
    Text,
    Button,
    Image,
    Panel,
}

/// Configurazione widget (serializzabile per tutti i runtime)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WidgetConfig {
    /// ID del widget padre (None = root della finestra)
    pub parent_id: Option<u64>,

    // === Layout ===
    pub layout: Option<LayoutType>,
    pub direction: Option<FlexDirection>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub gap: Option<f32>,

    // === Dimensioni ===
    pub width: Option<SizeValue>,
    pub height: Option<SizeValue>,
    pub min_width: Option<SizeValue>,
    pub max_width: Option<SizeValue>,
    pub min_height: Option<SizeValue>,
    pub max_height: Option<SizeValue>,

    // === Spacing ===
    pub margin: Option<EdgeInsets>,
    pub padding: Option<EdgeInsets>,

    // === Aspetto e Trasparenza ===
    pub background_color: Option<ColorValue>,    // RGBA con alpha
    pub border_color: Option<ColorValue>,        // RGBA con alpha
    pub border_width: Option<EdgeInsets>,
    pub border_radius: Option<f32>,
    pub opacity: Option<f32>,                    // 0.0-1.0, opacità globale del widget
    pub blend_mode: Option<BlendMode>,           // Modalità di fusione

    // === Background Image ===
    pub background_image: Option<ImageConfig>,   // Immagine di sfondo (alternativa a background_color)

    // === Text e Font ===
    pub content: Option<String>,
    pub font: Option<FontConfig>,                // Configurazione font completa
    pub font_color: Option<ColorValue>,          // RGBA con alpha
    pub text_align: Option<TextAlign>,
    pub text_shadow: Option<ShadowConfig>,       // Ombra del testo

    // === Button ===
    pub label: Option<String>,
    pub hover_color: Option<ColorValue>,         // RGBA con alpha
    pub pressed_color: Option<ColorValue>,       // RGBA con alpha
    pub disabled: Option<bool>,
    pub disabled_color: Option<ColorValue>,      // Colore quando disabilitato

    // === Image Widget ===
    pub image: Option<ImageConfig>,              // Per widget di tipo Image
}

/// Configurazione immagine (per background o widget Image)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageConfig {
    /// Percorso dell'asset immagine (relativo alla cartella mod o asset)
    pub path: String,
    /// Modalità di scala
    pub scale_mode: Option<ImageScaleMode>,
    /// Tint color (moltiplicato con i pixel dell'immagine)
    pub tint: Option<ColorValue>,
    /// Opacità dell'immagine (0.0-1.0)
    pub opacity: Option<f32>,
    /// Flip orizzontale
    pub flip_x: Option<bool>,
    /// Flip verticale
    pub flip_y: Option<bool>,
    /// Regione dell'immagine da mostrare (per sprite sheets)
    pub source_rect: Option<RectValue>,
}

/// Configurazione font
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontConfig {
    /// Nome o percorso del font (es. "Roboto", "fonts/custom.ttf")
    pub family: String,
    /// Dimensione in pixel
    pub size: f32,
    /// Peso del font
    pub weight: Option<FontWeight>,
    /// Stile (normale, italico)
    pub style: Option<FontStyle>,
    /// Spaziatura tra caratteri
    pub letter_spacing: Option<f32>,
    /// Altezza della linea (moltiplicatore)
    pub line_height: Option<f32>,
}

/// Configurazione ombra (per testo o widget)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShadowConfig {
    pub color: ColorValue,           // RGBA con alpha
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: Option<f32>,
}

/// Rettangolo (per source_rect delle immagini)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RectValue {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Valore di dimensione (supporta px, %, auto)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SizeValue {
    Px(f32),
    Percent(f32),
    Auto,
}

/// Insets per margin/padding/border (top, right, bottom, left)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Colore (RGBA 0.0-1.0) con supporto completo per trasparenza
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorValue {
    pub r: f32,    // 0.0-1.0
    pub g: f32,    // 0.0-1.0
    pub b: f32,    // 0.0-1.0
    pub a: f32,    // 0.0-1.0 (0 = trasparente, 1 = opaco)
}

impl ColorValue {
    /// Crea colore da hex string (es. "#FF0000", "#FF0000FF", "rgba(255,0,0,0.5)")
    pub fn from_hex(hex: &str) -> Result<Self, ColorParseError>;

    /// Crea colore con alpha specifico
    pub fn with_alpha(self, alpha: f32) -> Self;

    /// Crea colore completamente trasparente
    pub fn transparent() -> Self { Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 } }

    /// Colori predefiniti
    pub fn white() -> Self { Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 } }
    pub fn black() -> Self { Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 } }
}

/// Modalità di fusione per effetti grafici avanzati
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    /// Normale (default) - alpha blending standard
    Normal,
    /// Moltiplica i colori (scurisce)
    Multiply,
    /// Schermo (schiarisce)
    Screen,
    /// Overlay (combinazione di multiply e screen)
    Overlay,
    /// Additivo (aggiunge luminosità)
    Add,
    /// Sottrae colore
    Subtract,
}

/// Modalità di scala per immagini
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageScaleMode {
    /// Scala per riempire mantenendo aspect ratio (può tagliare)
    Fill,
    /// Scala per contenere mantenendo aspect ratio (può lasciare spazi)
    Fit,
    /// Scala per riempire ignorando aspect ratio
    Stretch,
    /// Nessuna scala, dimensione originale
    None,
    /// Ripete l'immagine come pattern (tile)
    Tile,
    /// 9-slice scaling per UI (preserva bordi)
    NineSlice {
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    },
}

/// Peso del font
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontWeight {
    Thin,       // 100
    Light,      // 300
    Regular,    // 400
    Medium,     // 500
    SemiBold,   // 600
    Bold,       // 700
    ExtraBold,  // 800
    Black,      // 900
}

/// Stile del font
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// Informazioni su un widget (restituito da query)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WidgetInfo {
    pub id: u64,
    pub window_id: u64,
    pub widget_type: WidgetType,
    pub parent_id: Option<u64>,
    pub children_ids: Vec<u64>,
}

/// Valore di proprietà dinamico (per update)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
    Color(ColorValue),
    Size(SizeValue),
}
```

### 4.3 API GraphicProxy (estesa per Widget)

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs

impl GraphicProxy {
    // === Widget Creation ===

    /// Crea un nuovo widget nella finestra specificata
    pub async fn create_widget(
        &self,
        window_id: u64,
        widget_type: WidgetType,
        config: WidgetConfig,
    ) -> Result<u64, GraphicError>;

    // === Widget Modification ===

    /// Aggiorna una proprietà del widget
    pub async fn update_widget_property(
        &self,
        widget_id: u64,
        property: &str,
        value: PropertyValue,
    ) -> Result<(), GraphicError>;

    /// Aggiorna più proprietà in una sola chiamata
    pub async fn update_widget_config(
        &self,
        widget_id: u64,
        config: WidgetConfig,
    ) -> Result<(), GraphicError>;

    // === Widget Hierarchy ===

    /// Sposta un widget sotto un nuovo parent
    pub async fn reparent_widget(
        &self,
        widget_id: u64,
        new_parent_id: Option<u64>,
    ) -> Result<(), GraphicError>;

    /// Distrugge un widget e tutti i suoi figli
    pub async fn destroy_widget(&self, widget_id: u64) -> Result<(), GraphicError>;

    /// Distrugge tutti i widget di una finestra
    pub async fn clear_window_widgets(&self, window_id: u64) -> Result<(), GraphicError>;

    // === Widget Query ===

    /// Ottiene informazioni su un widget
    pub fn get_widget_info(&self, widget_id: u64) -> Option<WidgetInfo>;

    /// Ottiene tutti i widget di una finestra
    pub fn get_window_widgets(&self, window_id: u64) -> Vec<WidgetInfo>;

    /// Ottiene il root widget di una finestra
    pub fn get_window_root_widget(&self, window_id: u64) -> Option<u64>;

    // === Event Subscription ===

    /// Registra interesse per eventi di un widget (click, hover, etc.)
    pub async fn subscribe_widget_events(
        &self,
        widget_id: u64,
        event_types: Vec<WidgetEventType>,
    ) -> Result<(), GraphicError>;

    /// Rimuove interesse per eventi di un widget
    pub async fn unsubscribe_widget_events(
        &self,
        widget_id: u64,
        event_types: Vec<WidgetEventType>,
    ) -> Result<(), GraphicError>;

    // === Asset Management (Font & Images) ===

    /// Carica un font custom da file
    /// Restituisce un handle che può essere usato in FontConfig.family
    pub async fn load_font(
        &self,
        path: &str,           // Percorso relativo alla cartella mod/assets
        alias: Option<&str>,  // Nome da usare per riferirsi al font (default: nome file)
    ) -> Result<String, GraphicError>;

    /// Precarica un'immagine (opzionale, per evitare lag al primo uso)
    pub async fn preload_image(
        &self,
        path: &str,
    ) -> Result<(), GraphicError>;

    /// Ottiene la lista dei font caricati
    pub fn get_loaded_fonts(&self) -> Vec<FontInfo>;

    /// Scarica un font dalla memoria
    pub async fn unload_font(&self, alias: &str) -> Result<(), GraphicError>;
}

/// Informazioni su un font caricato
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontInfo {
    pub alias: String,
    pub path: String,
    pub family_name: Option<String>,  // Nome interno del font se disponibile
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetEventType {
    Click,
    Hover,
    Focus,
}
```

### 4.4 Eventi Widget

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/events.rs

/// Eventi widget (inviati da Bevy al worker thread)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WidgetEvent {
    Created {
        window_id: u64,
        widget_id: u64,
        widget_type: WidgetType,
    },
    Destroyed {
        window_id: u64,
        widget_id: u64,
    },
    Clicked {
        window_id: u64,
        widget_id: u64,
        x: f32,
        y: f32,
        button: MouseButton,
    },
    Hovered {
        window_id: u64,
        widget_id: u64,
        entered: bool,
        x: f32,
        y: f32,
    },
    Focused {
        window_id: u64,
        widget_id: u64,
        focused: bool,
    },
}
```

---

## 5. Binding per Runtime Specifici

Ogni runtime adapter implementa i binding verso l'API core. La logica core rimane in Rust, i binding traducono solo i tipi.

### 5.1 Architettura Binding

```
┌─────────────────────────────────────────────────────────────────┐
│                     GraphicProxy (Rust Core)                     │
│  • create_widget(), update_widget_property(), destroy_widget()  │
│  • Nessuna dipendenza da linguaggi specifici                    │
└─────────────────────────────────────────────────────────────────┘
                              ↑
        ┌─────────────────────┼─────────────────────┐
        ↓                     ↓                     ↓
┌───────────────┐     ┌───────────────┐     ┌───────────────┐
│ JS Adapter    │     │ Lua Adapter   │     │ C# Adapter    │
│ (rquickjs)    │     │ (mlua)        │     │ (dotnet)      │
│               │     │               │     │               │
│ WidgetJS      │     │ WidgetLua     │     │ WidgetCS      │
│ .onClick(fn)  │     │ :onClick(fn)  │     │ .OnClick(fn)  │
└───────────────┘     └───────────────┘     └───────────────┘
```

### 5.2 JavaScript Binding (Esempio Attuale)

```javascript
// === Caricamento Font Custom ===
await graphic.loadFont("fonts/Roboto-Bold.ttf", "roboto-bold");
await graphic.loadFont("fonts/GameFont.otf", "game-font");

// === Creazione UI con trasparenza e immagini ===
const mainPanel = await window.createWidget("panel", {
    width: "100%",
    height: "100%",
    // Background semitrasparente
    backgroundColor: "rgba(0, 0, 0, 0.7)",  // Nero con 70% opacità
    // Oppure immagine di sfondo
    backgroundImage: {
        path: "textures/background.png",
        scaleMode: "fill",
        opacity: 0.8
    }
});

// === Testo con font custom e ombra ===
const title = await window.createWidget("text", {
    parent: mainPanel.id,
    content: "Game Title",
    font: {
        family: "game-font",  // Font caricato sopra
        size: 48,
        weight: "bold"
    },
    fontColor: "rgba(255, 255, 255, 0.9)",  // Bianco quasi opaco
    textShadow: {
        color: "rgba(0, 0, 0, 0.5)",
        offsetX: 2,
        offsetY: 2,
        blurRadius: 4
    },
    textAlign: "center"
});

// === Button con stati trasparenti ===
const button = await window.createWidget("button", {
    parent: mainPanel.id,
    label: "Start Game",
    font: { family: "roboto-bold", size: 18 },
    backgroundColor: "rgba(74, 144, 217, 0.8)",    // Blu semitrasparente
    hoverColor: "rgba(91, 160, 233, 0.9)",         // Più luminoso al hover
    pressedColor: "rgba(58, 128, 201, 1.0)",       // Opaco quando premuto
    borderRadius: 8,
    padding: [12, 24, 12, 24]
});

// === Widget Image per icone/sprite ===
const icon = await window.createWidget("image", {
    parent: button.id,
    image: {
        path: "icons/play.png",
        tint: "rgba(255, 255, 255, 0.9)",  // Tinta bianca
        scaleMode: "fit"
    },
    width: 24,
    height: 24
});

// === Panel con 9-slice per bordi ===
const dialogBox = await window.createWidget("panel", {
    backgroundImage: {
        path: "ui/dialog-frame.png",
        scaleMode: {
            type: "nineSlice",
            top: 16, right: 16, bottom: 16, left: 16
        }
    },
    opacity: 0.95  // Opacità globale del widget
});

button.onClick((event) => {
    console.log(`Clicked at ${event.x}, ${event.y}`);
});

await button.setProperty("label", "Clicked!");
await button.destroy();
```

### 5.3 Lua Binding (Esempio Futuro)

```lua
-- Uso in Lua
local button = window:createWidget("button", {
    label = "Click Me",
    backgroundColor = "#4A90D9"
})

button:onClick(function(event)
    print("Clicked at " .. event.x .. ", " .. event.y)
end)

button:setProperty("label", "Clicked!")
button:destroy()
```

### 5.4 C# Binding (Esempio Futuro)

```csharp
// Uso in C#
var button = await window.CreateWidget("button", new WidgetConfig {
    Label = "Click Me",
    BackgroundColor = "#4A90D9"
});

button.OnClick += (sender, e) => {
    Console.WriteLine($"Clicked at {e.X}, {e.Y}");
};

await button.SetProperty("label", "Clicked!");
await button.Destroy();
```

### 5.5 Rust Native Mod (Esempio Futuro)

```rust
// Uso in Rust (mod nativo)
let button = window.create_widget(WidgetType::Button, WidgetConfig {
    label: Some("Click Me".into()),
    background_color: Some(ColorValue::from_hex("#4A90D9")),
    ..Default::default()
}).await?;

// Callback via evento
system.on_widget_event(button.id, |event| {
    if let WidgetEvent::Clicked { x, y, .. } = event {
        println!("Clicked at {}, {}", x, y);
    }
});

button.set_property("label", PropertyValue::String("Clicked!".into())).await?;
button.destroy().await?;
```

### 5.6 Gestione Callback Cross-Language

Le callback sono gestite tramite il sistema eventi esistente, non come function pointers:

```rust
// Nel runtime adapter (es. JS)
impl WidgetJS {
    pub fn on_click(&self, ctx: Ctx, handler: Function) -> Result<()> {
        // 1. Registra interesse presso GraphicProxy
        self.graphic_proxy.subscribe_widget_events(
            self.widget_id,
            vec![WidgetEventType::Click]
        );

        // 2. Memorizza handler nel registry locale del runtime
        self.callback_registry.register(
            self.widget_id,
            "click",
            handler.into_persistent()
        );

        Ok(())
    }
}

// Quando arriva l'evento da Bevy:
fn dispatch_widget_event(event: WidgetEvent, runtime: &mut JsRuntime) {
    match event {
        WidgetEvent::Clicked { widget_id, x, y, button } => {
            if let Some(handler) = runtime.callback_registry.get(widget_id, "click") {
                let event_obj = create_js_click_event(x, y, button);
                handler.call((event_obj,));
            }
        }
        // ...
    }
}
```

---

## 6. Implementazione Rust Core

### 6.1 Nuovi Comandi GraphicCommand

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/commands.rs

pub enum GraphicCommand {
    // ... comandi esistenti ...

    // Widget commands
    CreateWidget {
        window_id: u64,
        widget_id: u64,
        parent_id: Option<u64>,
        widget_type: WidgetType,
        config: WidgetConfig,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    UpdateWidget {
        widget_id: u64,
        property: String,
        value: PropertyValue,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    DestroyWidget {
        widget_id: u64,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    ReparentWidget {
        widget_id: u64,
        new_parent_id: Option<u64>,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    QueryWidgets {
        window_id: u64,
        filter: WidgetFilter,
        response_tx: oneshot::Sender<Result<Vec<WidgetInfo>, String>>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WidgetConfig {
    // Layout
    pub layout: Option<LayoutType>,
    pub direction: Option<FlexDirection>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub gap: Option<f32>,

    // Dimensioni
    pub width: Option<Val>,
    pub height: Option<Val>,
    pub min_width: Option<Val>,
    pub max_width: Option<Val>,
    pub min_height: Option<Val>,
    pub max_height: Option<Val>,

    // Spacing
    pub margin: Option<UiRect>,
    pub padding: Option<UiRect>,

    // Aspetto
    pub background_color: Option<Color>,
    pub border_color: Option<Color>,
    pub border_width: Option<UiRect>,
    pub border_radius: Option<BorderRadius>,

    // Text
    pub content: Option<String>,
    pub font_size: Option<f32>,
    pub font_color: Option<Color>,
    pub text_align: Option<JustifyText>,

    // Button
    pub label: Option<String>,
    pub hover_color: Option<Color>,
    pub pressed_color: Option<Color>,
    pub disabled: Option<bool>,

    // Image
    pub image_path: Option<String>,
    pub scale_mode: Option<ImageScaleMode>,
}

#[derive(Clone, Debug)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
    Color(Color),
    Val(Val),
}
```

### 6.2 Nuovi Eventi GraphicEvent

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/events.rs

pub enum GraphicEvent {
    // ... eventi esistenti ...

    // Widget events
    WidgetCreated {
        window_id: u64,
        widget_id: u64,
        widget_type: WidgetType,
    },
    WidgetDestroyed {
        window_id: u64,
        widget_id: u64,
    },
    WidgetClicked {
        window_id: u64,
        widget_id: u64,
        x: f32,
        y: f32,
        button: MouseButton,
    },
    WidgetHovered {
        window_id: u64,
        widget_id: u64,
        entered: bool,
        x: f32,
        y: f32,
    },
    WidgetFocused {
        window_id: u64,
        widget_id: u64,
        focused: bool,
    },
    WidgetInteractionChanged {
        window_id: u64,
        widget_id: u64,
        interaction: Interaction,
    },
}
```

### 6.3 Sistema Bevy per Widget

```rust
// In apps/stam_client/src/engines/bevy.rs

/// Registry dei widget per window
#[derive(Resource, Default)]
pub struct WidgetRegistry {
    widgets: HashMap<u64, Entity>,
    widget_to_window: HashMap<u64, u64>,
    window_root_nodes: HashMap<u64, Entity>,
}

/// Marker component per widget Staminal
#[derive(Component)]
pub struct StamWidget {
    pub id: u64,
    pub window_id: u64,
    pub widget_type: WidgetType,
}

/// Component per tracciare callback registrate
#[derive(Component, Default)]
pub struct WidgetCallbacks {
    pub on_click: bool,
    pub on_hover: bool,
    pub on_focus: bool,
}

/// Component per colori hover/pressed dei button
#[derive(Component)]
pub struct ButtonColors {
    pub normal: Color,
    pub hovered: Color,
    pub pressed: Color,
}

/// Sistema per processare comandi widget
fn process_widget_commands(
    mut commands: Commands,
    mut widget_registry: ResMut<WidgetRegistry>,
    window_registry: Res<WindowRegistry>,
    cmd_rx: Res<CommandReceiverRes>,
    event_tx: Res<EventSenderRes>,
    mut query: Query<&mut Node>,
    // ... altre query necessarie
) {
    while let Ok(cmd) = cmd_rx.0.try_recv() {
        match cmd {
            GraphicCommand::CreateWidget {
                window_id, widget_id, parent_id, widget_type, config, response_tx
            } => {
                // Creare l'entità widget appropriata
                let entity = create_widget_entity(
                    &mut commands,
                    &widget_registry,
                    &window_registry,
                    window_id,
                    widget_id,
                    parent_id,
                    widget_type,
                    config,
                );

                widget_registry.widgets.insert(widget_id, entity);
                widget_registry.widget_to_window.insert(widget_id, window_id);

                let _ = response_tx.send(Ok(()));
                let _ = event_tx.0.try_send(GraphicEvent::WidgetCreated {
                    window_id,
                    widget_id,
                    widget_type,
                });
            }
            // ... altri comandi
        }
    }
}

/// Sistema per gestire interazioni widget
fn handle_widget_interactions(
    mut interaction_query: Query<
        (&Interaction, &StamWidget, &WidgetCallbacks, Option<&ButtonColors>, &mut BackgroundColor),
        Changed<Interaction>
    >,
    event_tx: Res<EventSenderRes>,
) {
    for (interaction, stam_widget, callbacks, button_colors, mut bg_color) in interaction_query.iter_mut() {
        // Aggiornare colore per button
        if let Some(colors) = button_colors {
            *bg_color = match *interaction {
                Interaction::Pressed => BackgroundColor(colors.pressed),
                Interaction::Hovered => BackgroundColor(colors.hovered),
                Interaction::None => BackgroundColor(colors.normal),
            };
        }

        // Inviare evento al worker thread
        if callbacks.on_click && *interaction == Interaction::Pressed {
            let _ = event_tx.0.try_send(GraphicEvent::WidgetClicked {
                window_id: stam_widget.window_id,
                widget_id: stam_widget.id,
                x: 0.0, // TODO: ottenere posizione reale
                y: 0.0,
                button: MouseButton::Left,
            });
        }

        if callbacks.on_hover {
            let entered = *interaction == Interaction::Hovered;
            let _ = event_tx.0.try_send(GraphicEvent::WidgetHovered {
                window_id: stam_widget.window_id,
                widget_id: stam_widget.id,
                entered,
                x: 0.0,
                y: 0.0,
            });
        }
    }
}

/// Funzione helper per creare entità widget
fn create_widget_entity(
    commands: &mut Commands,
    widget_registry: &WidgetRegistry,
    window_registry: &WindowRegistry,
    window_id: u64,
    widget_id: u64,
    parent_id: Option<u64>,
    widget_type: WidgetType,
    config: WidgetConfig,
) -> Entity {
    // Determinare parent entity
    let parent_entity = match parent_id {
        Some(pid) => widget_registry.widgets.get(&pid).copied(),
        None => widget_registry.window_root_nodes.get(&window_id).copied(),
    };

    // Costruire Node base
    let node = build_node_from_config(&config);

    // Creare entità in base al tipo
    match widget_type {
        WidgetType::Container => {
            let mut entity_commands = commands.spawn((
                node,
                StamWidget { id: widget_id, window_id, widget_type },
                WidgetCallbacks::default(),
            ));

            if let Some(color) = config.background_color {
                entity_commands.insert(BackgroundColor(color));
            }

            if let Some(parent) = parent_entity {
                entity_commands.set_parent(parent);
            }

            entity_commands.id()
        }
        WidgetType::Text => {
            let content = config.content.unwrap_or_default();
            let font_size = config.font_size.unwrap_or(16.0);
            let color = config.font_color.unwrap_or(Color::WHITE);

            let mut entity_commands = commands.spawn((
                node,
                Text::new(content),
                TextColor(color),
                TextFont { font_size, ..default() },
                StamWidget { id: widget_id, window_id, widget_type },
                WidgetCallbacks::default(),
            ));

            if let Some(parent) = parent_entity {
                entity_commands.set_parent(parent);
            }

            entity_commands.id()
        }
        WidgetType::Button => {
            let label = config.label.clone().unwrap_or_default();
            let normal = config.background_color.unwrap_or(Color::srgb(0.3, 0.3, 0.3));
            let hovered = config.hover_color.unwrap_or(Color::srgb(0.4, 0.4, 0.4));
            let pressed = config.pressed_color.unwrap_or(Color::srgb(0.2, 0.2, 0.2));

            let mut entity_commands = commands.spawn((
                node,
                Button,
                BackgroundColor(normal),
                ButtonColors { normal, hovered, pressed },
                StamWidget { id: widget_id, window_id, widget_type },
                WidgetCallbacks { on_click: true, on_hover: true, ..default() },
            ));

            // Aggiungere label come figlio
            entity_commands.with_children(|parent| {
                parent.spawn((
                    Text::new(label),
                    TextColor(Color::WHITE),
                    TextFont { font_size: config.font_size.unwrap_or(16.0), ..default() },
                ));
            });

            if let Some(parent) = parent_entity {
                entity_commands.set_parent(parent);
            }

            entity_commands.id()
        }
        // ... altri tipi
    }
}
```

### 6.4 Estensione GraphicProxy

```rust
// In apps/shared/stam_mod_runtimes/src/api/graphic/proxy.rs

impl GraphicProxy {
    // ... metodi esistenti ...

    pub async fn create_widget(
        &self,
        window_id: u64,
        widget_type: WidgetType,
        parent_id: Option<u64>,
        config: WidgetConfig,
    ) -> Result<u64, GraphicError> {
        if !self.available {
            return Err(GraphicError::NotAvailable(
                "Widget creation is not available on the server".into()
            ));
        }

        let cmd_tx = self.command_tx.read().await;
        let cmd_tx = cmd_tx.as_ref().ok_or(GraphicError::NoEngineEnabled)?;

        let widget_id = self.next_widget_id.fetch_add(1, Ordering::SeqCst);
        let (response_tx, response_rx) = oneshot::channel();

        cmd_tx.send(GraphicCommand::CreateWidget {
            window_id,
            widget_id,
            parent_id,
            widget_type,
            config,
            response_tx,
        }).map_err(|_| GraphicError::ChannelClosed)?;

        response_rx.await
            .map_err(|_| GraphicError::ResponseTimeout)?
            .map_err(GraphicError::CommandFailed)?;

        Ok(widget_id)
    }

    pub async fn update_widget(
        &self,
        widget_id: u64,
        property: String,
        value: PropertyValue,
    ) -> Result<(), GraphicError> {
        // ... implementazione simile ...
    }

    pub async fn destroy_widget(&self, widget_id: u64) -> Result<(), GraphicError> {
        // ... implementazione simile ...
    }
}
```

### 6.5 Binding JavaScript per Widget (Esempio)

```rust
// In apps/shared/stam_mod_runtimes/src/adapters/js/bindings.rs

/// Widget JavaScript class
#[derive(Clone, Trace)]
#[rquickjs::class]
pub struct WidgetJS {
    #[qjs(skip_trace)]
    widget_id: u64,
    #[qjs(skip_trace)]
    window_id: u64,
    #[qjs(skip_trace)]
    widget_type: WidgetType,
    #[qjs(skip_trace)]
    graphic_proxy: Arc<GraphicProxy>,
}

#[rquickjs::methods]
impl WidgetJS {
    #[qjs(get)]
    pub fn id(&self) -> u64 {
        self.widget_id
    }

    #[qjs(get, rename = "type")]
    pub fn widget_type(&self) -> String {
        self.widget_type.to_string()
    }

    #[qjs(get, rename = "windowId")]
    pub fn window_id(&self) -> u64 {
        self.window_id
    }

    #[qjs(rename = "setProperty")]
    pub async fn set_property<'js>(
        &self,
        ctx: Ctx<'js>,
        name: String,
        value: Value<'js>,
    ) -> rquickjs::Result<()> {
        let property_value = js_value_to_property_value(&ctx, value)?;

        self.graphic_proxy
            .update_widget(self.widget_id, name, property_value)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e.to_string())?.into()))?;

        Ok(())
    }

    #[qjs(rename = "destroy")]
    pub async fn destroy<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<()> {
        self.graphic_proxy
            .destroy_widget(self.widget_id)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e.to_string())?.into()))?;

        Ok(())
    }

    // Callback methods - registrano l'interesse nel GraphicProxy
    #[qjs(rename = "onClick")]
    pub fn on_click<'js>(&self, ctx: Ctx<'js>, handler: Function<'js>) -> rquickjs::Result<()> {
        // Registrare callback nel sistema eventi JavaScript
        let event_name = format!("widget:{}:click", self.widget_id);
        // ... registrazione handler ...
        Ok(())
    }
}

// Estensione WindowJS per widget
#[rquickjs::methods]
impl WindowJS {
    // ... metodi esistenti ...

    #[qjs(rename = "createWidget")]
    pub async fn create_widget<'js>(
        &self,
        ctx: Ctx<'js>,
        widget_type: String,
        config: Object<'js>,
    ) -> rquickjs::Result<WidgetJS> {
        let wtype = WidgetType::from_str(&widget_type)
            .map_err(|_| ctx.throw(
                rquickjs::String::from_str(ctx.clone(), &format!("Unknown widget type: {}", widget_type))?.into()
            ))?;

        let parent_id = config.get::<_, Option<u64>>("parent")?;
        let widget_config = parse_widget_config(&ctx, &config)?;

        let widget_id = self.graphic_proxy
            .create_widget(self.window_id, wtype, parent_id, widget_config)
            .await
            .map_err(|e| ctx.throw(rquickjs::String::from_str(ctx.clone(), &e.to_string())?.into()))?;

        Ok(WidgetJS {
            widget_id,
            window_id: self.window_id,
            widget_type: wtype,
            graphic_proxy: Arc::clone(&self.graphic_proxy),
        })
    }

    #[qjs(rename = "getWidget")]
    pub fn get_widget(&self, widget_id: u64) -> Option<WidgetJS> {
        // ... implementazione ...
    }

    #[qjs(rename = "clearWidgets")]
    pub async fn clear_widgets<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<()> {
        // ... implementazione ...
    }
}
```

---

## 7. Sistema di Eventi e Callback

### 7.1 Flusso Eventi Widget (Language-Agnostic)

```
┌─────────────────────────────────────────────────────┐
│                   Bevy Main Thread                  │
│  ┌──────────────────────────────────────────────┐  │
│  │ handle_widget_interactions()                 │  │
│  │  • Detecta Interaction changes               │  │
│  │  • Invia WidgetEvent::Clicked                │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
           ↓ event_tx (tokio::mpsc)
┌─────────────────────────────────────────────────────┐
│                  Worker Thread                      │
│  ┌──────────────────────────────────────────────┐  │
│  │ Main Event Loop                              │  │
│  │  • Riceve WidgetEvent                        │  │
│  │  • Chiama RuntimeManager::dispatch_event()   │  │
│  └──────────────────────────────────────────────┘  │
│           ↓                                         │
│  ┌──────────────────────────────────────────────┐  │
│  │ Runtime Adapter (JS, Lua, C#, etc.)          │  │
│  │  • Trova callback registrate per widget_id   │  │
│  │  • Esegue handler nel linguaggio specifico   │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 7.2 Trait per Callback Registry (Language-Agnostic)

Ogni runtime adapter implementa un proprio callback registry, ma tutti seguono lo stesso pattern:

```rust
// In stam_mod_runtimes/src/api/graphic/callbacks.rs

/// Trait che ogni runtime adapter deve implementare per gestire callback widget
pub trait WidgetCallbackDispatcher: Send + Sync {
    /// Registra interesse per un tipo di evento su un widget
    fn subscribe(&mut self, widget_id: u64, event_type: WidgetEventType);

    /// Rimuove interesse per un tipo di evento
    fn unsubscribe(&mut self, widget_id: u64, event_type: WidgetEventType);

    /// Dispatch di un evento widget - il runtime specifico gestisce la callback
    fn dispatch(&self, event: &WidgetEvent);

    /// Pulisce tutte le callback per un widget (chiamato quando il widget viene distrutto)
    fn cleanup_widget(&mut self, widget_id: u64);
}

/// Registry base condiviso (solo tracking delle subscription, no callbacks)
#[derive(Default)]
pub struct WidgetSubscriptionRegistry {
    subscriptions: HashMap<u64, HashSet<WidgetEventType>>,
}

impl WidgetSubscriptionRegistry {
    pub fn subscribe(&mut self, widget_id: u64, event_type: WidgetEventType) {
        self.subscriptions
            .entry(widget_id)
            .or_default()
            .insert(event_type);
    }

    pub fn is_subscribed(&self, widget_id: u64, event_type: WidgetEventType) -> bool {
        self.subscriptions
            .get(&widget_id)
            .map(|set| set.contains(&event_type))
            .unwrap_or(false)
    }
}
```

### 7.3 Dispatch Eventi nel Main Loop

```rust
// In apps/stam_client/src/main.rs

fn handle_graphic_event(
    event: GraphicEvent,
    runtime_manager: &mut RuntimeManager,
    widget_callback_registry: &WidgetCallbackRegistry,
) {
    match event {
        GraphicEvent::WidgetClicked { window_id, widget_id, x, y, button } => {
            let click_event = ClickEvent { widget_id, window_id, x, y, button };

            // Dispatch al JavaScript runtime
            runtime_manager.dispatch_widget_event(
                "click",
                widget_id,
                click_event,
            );
        }
        GraphicEvent::WidgetHovered { window_id, widget_id, entered, x, y } => {
            let hover_event = HoverEvent { widget_id, window_id, entered, x, y };

            runtime_manager.dispatch_widget_event(
                "hover",
                widget_id,
                hover_event,
            );
        }
        // ... altri eventi
    }
}
```

---

## 8. Piano di Implementazione

### Fase 1: Core API (Language-Agnostic) - Priorità Alta

#### 1.1 Definizione Tipi Core
- [ ] Creare `widget.rs` in `stam_mod_runtimes/src/api/graphic/`
  - [ ] `WidgetType` enum
  - [ ] `WidgetConfig` struct (serializzabile)
  - [ ] `SizeValue`, `EdgeInsets` (tipi layout)
  - [ ] `ColorValue` con supporto RGBA e parsing hex/rgba()
  - [ ] `BlendMode` enum per effetti di fusione
  - [ ] `ImageConfig` struct (path, scaleMode, tint, opacity, flip, sourceRect)
  - [ ] `ImageScaleMode` enum (Fill, Fit, Stretch, None, Tile, NineSlice)
  - [ ] `FontConfig` struct (family, size, weight, style, letterSpacing, lineHeight)
  - [ ] `FontWeight`, `FontStyle` enum
  - [ ] `ShadowConfig` struct per ombre testo/widget
  - [ ] `RectValue` per sprite sheet regions
  - [ ] `PropertyValue` enum per aggiornamenti dinamici
  - [ ] `WidgetInfo` struct per query
  - [ ] `WidgetEventType` enum

#### 1.2 Comandi e Eventi
- [ ] Estendere `GraphicCommand` in `commands.rs`
  - [ ] `CreateWidget`
  - [ ] `UpdateWidgetProperty`
  - [ ] `UpdateWidgetConfig`
  - [ ] `DestroyWidget`
  - [ ] `ReparentWidget`
  - [ ] `ClearWindowWidgets`
  - [ ] `SubscribeWidgetEvents`
  - [ ] `UnsubscribeWidgetEvents`
  - [ ] `LoadFont` (path, alias)
  - [ ] `UnloadFont` (alias)
  - [ ] `PreloadImage` (path)
- [ ] Creare `WidgetEvent` enum in `events.rs`
  - [ ] `Created`, `Destroyed`
  - [ ] `Clicked`, `Hovered`, `Focused`

#### 1.3 Estensione GraphicProxy
- [ ] Metodi async per widget in `proxy.rs`
  - [ ] `create_widget()`
  - [ ] `update_widget_property()`, `update_widget_config()`
  - [ ] `destroy_widget()`, `clear_window_widgets()`
  - [ ] `reparent_widget()`
  - [ ] `subscribe_widget_events()`, `unsubscribe_widget_events()`
- [ ] Metodi per asset management
  - [ ] `load_font()`, `unload_font()`, `get_loaded_fonts()`
  - [ ] `preload_image()`
- [ ] Metodi sync per query
  - [ ] `get_widget_info()`, `get_window_widgets()`
  - [ ] `get_window_root_widget()`
- [ ] Aggiungere `next_widget_id: AtomicU64`
- [ ] Aggiungere `widgets: Arc<RwLock<HashMap<u64, WidgetInfo>>>`
- [ ] Aggiungere `loaded_fonts: Arc<RwLock<HashMap<String, FontInfo>>>`

### Fase 2: Implementazione Bevy - Priorità Alta

#### 2.1 Widget Registry e Asset Registry
- [ ] Creare `WidgetRegistry` resource
- [ ] Creare `FontRegistry` resource (alias → Handle<Font>)
- [ ] Creare `ImageCache` resource (path → Handle<Image>)
- [ ] Creare `StamWidget` component (marker)
- [ ] Creare `WidgetEventSubscriptions` component
- [ ] Creare `ButtonColors` component (normal, hover, pressed, disabled)
- [ ] Creare `WidgetOpacity` component (per gestione trasparenza gerarchica)

#### 2.2 Sistema Comandi Widget
- [ ] Implementare handling in `process_commands` o nuovo system
- [ ] `create_widget_entity()` helper per ogni `WidgetType`
- [ ] Gestione gerarchia parent-child con Bevy relations
- [ ] Handling comandi `LoadFont`, `UnloadFont`, `PreloadImage`

#### 2.3 Sistema Rendering Avanzato
- [ ] Sistema per applicare `opacity` a widget e figli
- [ ] Sistema per applicare `BlendMode` (richiede shader custom o bevy_blend_modes)
- [ ] Sistema per rendering `background_image` con tutte le opzioni
- [ ] Sistema per 9-slice scaling

#### 2.4 Sistema Interazioni
- [ ] `handle_widget_interactions` system
- [ ] Query su `Changed<Interaction>` per widget
- [ ] Invio eventi solo per widget con subscription attive
- [ ] Gestione automatica colori button (incluso disabled)

### Fase 3: Binding JavaScript - Priorità Alta

#### 3.1 Callback Registry JS-specific
- [ ] Creare `JsWidgetCallbackRegistry` in `adapters/js/`
- [ ] Implementare `WidgetCallbackDispatcher` trait
- [ ] Gestire `PersistentFunction` per callback

#### 3.2 WidgetJS Class
- [ ] Proprietà: `id`, `type`, `windowId`
- [ ] Metodi: `setProperty()`, `destroy()`
- [ ] Callback: `onClick()`, `onHover()`, `onFocus()`
- [ ] Rimozione: `removeOnClick()`, `removeOnHover()`, `removeOnFocus()`

#### 3.3 Estensione WindowJS
- [ ] `createWidget(type, config)`
- [ ] `getWidget(id)`, `getWidgetsByType(type)`
- [ ] `getRootWidget()`, `clearWidgets()`

#### 3.4 Estensione GraphicJS
- [ ] `loadFont(path, alias)` - carica font custom
- [ ] `unloadFont(alias)` - scarica font
- [ ] `getLoadedFonts()` - lista font caricati
- [ ] `preloadImage(path)` - precarica immagine

#### 3.5 Parsing Colori e Config
- [ ] Parser per colori: "#RGB", "#RGBA", "#RRGGBB", "#RRGGBBAA", "rgba(r,g,b,a)"
- [ ] Parser per `FontConfig` da oggetto JS
- [ ] Parser per `ImageConfig` da oggetto JS
- [ ] Parser per `ShadowConfig` da oggetto JS

### Fase 4: Widget Specifici - Priorità Media

#### 4.1 Container Widget
- [ ] Flex layout (direction, justify, align, gap)
- [ ] Grid layout base (rows, columns)
- [ ] Background color con alpha
- [ ] Background image con tutte le opzioni
- [ ] Opacity globale

#### 4.2 Text Widget
- [ ] Content, fontColor con alpha
- [ ] FontConfig completo (family, size, weight, style)
- [ ] Letter spacing e line height
- [ ] TextAlign
- [ ] Text shadow con blur
- [ ] Update dinamico content

#### 4.3 Button Widget
- [ ] Label con FontConfig
- [ ] Colori con alpha: normal, hover, pressed, disabled
- [ ] Background image per stati
- [ ] Disabled state
- [ ] Interazione automatica
- [ ] Border radius

#### 4.4 Panel Widget
- [ ] Background color con alpha
- [ ] Background image (fill, tile, 9-slice)
- [ ] Border (color con alpha, width, radius)
- [ ] Opacity globale

#### 4.5 Image Widget
- [ ] Caricamento da asset path
- [ ] Scale modes (Fill, Fit, Stretch, None, Tile, NineSlice)
- [ ] Tint color con alpha
- [ ] Opacity
- [ ] Flip X/Y
- [ ] Source rect per sprite sheets

### Fase 5: Predisposizione Altri Runtime - Priorità Bassa

#### 5.1 Documentazione Binding
- [ ] Documentare come implementare `WidgetCallbackDispatcher`
- [ ] Template per nuovo runtime adapter

#### 5.2 Trait Bounds
- [ ] Verificare che tutti i tipi core siano `Serialize + Deserialize`
- [ ] Verificare thread-safety (`Send + Sync`)

### Fase 6: Testing e Documentazione - Priorità Media

#### 6.1 Test Manuali
- [ ] Creare mod demo con UI complessa
- [ ] Test creazione/distruzione widget
- [ ] Test callback su tutti i tipi
- [ ] Test gerarchia (parent-child, reparent)

#### 6.2 Documentazione
- [ ] API reference per tipi core
- [ ] Esempi per JavaScript
- [ ] Guida per implementare binding in altri linguaggi

---

## Riferimenti

### Documentazione Bevy UI
- [Bevy Window Struct](https://docs.rs/bevy/latest/bevy/window/struct.Window.html)
- [Bevy UI Overview](https://taintedcoders.com/bevy/ui)
- [Bevy Widgets Discussion](https://github.com/bevyengine/bevy/discussions/5604)

### Librerie Alternative (per riferimento futuro)
- [bevy_egui](https://docs.rs/bevy_egui/latest/bevy_egui/) - Integrazione Egui
- [Sickle UI](https://github.com/UmbraLuminworksai/sickle_ui) - UI ergonomica per Bevy

### Pattern Architetturali
- **Language-agnostic core**: API definita in Rust, binding per ogni linguaggio
- **Entity-based widgets**: Widget sono entità ECS con component marker
- **Command-Event pattern**: Comunicazione asincrona tra thread
- **Proxy pattern**: GraphicProxy media tra tutti i runtime e Bevy
- **ID-based references**: Widget referenziati tramite ID, non puntatori
- **Subscription-based events**: Callback registrate come interesse per eventi
- **Asset caching**: Font e immagini caricati una volta, referenziati per alias/path

### Note Tecniche su Trasparenza e Rendering

#### Alpha Blending
- Tutti i colori supportano canale alpha (0.0 = trasparente, 1.0 = opaco)
- `opacity` widget si moltiplica con alpha dei colori figli
- Bevy usa premultiplied alpha per default

#### BlendMode
- Richiede shader custom o integrazione con `bevy_blend_modes`
- `Normal` è l'unico mode supportato nativamente da Bevy UI
- Altri mode potrebbero richiedere render-to-texture

#### Font Loading
- Bevy supporta .ttf e .otf
- Font caricati tramite AssetServer
- Alias permette riferimento semplice nei widget

#### 9-Slice Scaling
- Bevy 0.15+ supporta `ImageScaleMode::Sliced` per UI
- Preserva angoli e bordi durante il ridimensionamento
- Ideale per dialog box, pannelli, bottoni stilizzati
