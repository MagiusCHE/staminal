# Client Game Engine : Bevy

## Obiettivo

Integrare il game engine Bevy nel client Staminal esistente ed esporre le API grafiche/UI attraverso il sistema `stam_mod_runtimes`, rendendole disponibili a tutti i linguaggi di scripting supportati (JavaScript, Lua, C#, Rust, C++).

## Contesto

### Struttura Esistente

Il progetto ha gia una struttura consolidata:

```
apps/
├── stam_client/src/
│   ├── main.rs              # Entry point attuale (tokio async)
│   ├── locale.rs            # Gestione localizzazione
│   ├── app_paths.rs         # Path dell'applicazione
│   └── mod_runtime/
│       ├── mod.rs
│       └── js_adapter.rs    # Adapter JavaScript specifico client
│
├── stam_server/src/
│   ├── main.rs
│   ├── config.rs
│   ├── mod_loader.rs
│   ├── client_manager.rs
│   ├── game_client.rs
│   └── primal_client.rs
│
└── shared/stam_mod_runtimes/src/
    ├── lib.rs               # RuntimeAdapter trait, RuntimeManager
    ├── runtime_type.rs      # RuntimeType enum
    ├── logging.rs
    ├── api/                  # API LANGUAGE-AGNOSTIC
    │   ├── mod.rs
    │   ├── console.rs       # ConsoleApi
    │   ├── process.rs       # AppApi
    │   ├── system.rs        # SystemApi, ModInfo
    │   ├── events.rs        # EventDispatcher, SystemEvents
    │   ├── locale.rs        # LocaleApi
    │   └── network.rs       # NetworkApi
    └── adapters/
        ├── mod.rs
        └── js/              # Adapter JavaScript
            ├── mod.rs
            ├── runtime.rs   # JsRuntimeAdapter
            ├── bindings.rs  # SystemJS, LocaleJS, NetworkJS
            ├── config.rs
            └── glue/        # Codice JS embedded
```

### Principi da Rispettare

1. **API in `stam_mod_runtimes/src/api/`**: Definizioni Rust pure, language-agnostic
2. **Binding in `adapters/{lang}/bindings.rs`**: Ogni linguaggio ha i propri binding
3. **Il client USA le API**: Non le definisce, le consuma
4. **Shared tra client e server**: Le API sono condivise, anche se il server non usa la UI

## Perche Bevy

| Caratteristica | Vantaggio |
|----------------|-----------|
| ECS (Entity Component System) | Architettura scalabile e performante |
| bevy_egui | Integrazione immediata con egui per UI |
| Asset system | Gestione risorse integrata |
| Cross-platform | Linux, macOS, Windows, Web (WASM) |
| Plugin system | Estensibile, modulare |
| Comunita attiva | Documentazione, esempi, supporto |

## Architettura Proposta

### Diagramma

```
┌─────────────────────────────────────────────────────────────────┐
│                        stam_client                               │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    main.rs (Bevy App)                     │   │
│  │  - Bevy event loop (sostituisce tokio::select! attuale)  │   │
│  │  - Integra networking, mod runtime, UI                    │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│         ┌────────────────────┼────────────────────┐             │
│         ▼                    ▼                    ▼             │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐       │
│  │ ui/         │     │ networking/ │     │ mod_runtime/│       │
│  │ bridge.rs   │     │ (esistente  │     │ (esistente) │       │
│  │ systems.rs  │     │  + Bevy)    │     │             │       │
│  │ render.rs   │     └─────────────┘     └─────────────┘       │
│  └─────────────┘                                                │
└─────────────────────────────────────────────────────────────────┘
                               │
                               ▼ (canali crossbeam)
┌─────────────────────────────────────────────────────────────────┐
│                     stam_mod_runtimes (SHARED)                   │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    api/ (ESISTENTE + NUOVE)               │   │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────────────┐ │   │
│  │  │console  │ │system   │ │network  │ │ ui.rs    [NUOVO]│ │   │
│  │  │process  │ │events   │ │locale   │ │ window.rs[NUOVO]│ │   │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────────────┘ │   │
│  └──────────────────────────────────────────────────────────┘   │
│                               │                                  │
│                               ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    adapters/js/bindings.rs                │   │
│  │  SystemJS, LocaleJS, NetworkJS + UiJS [NUOVO]             │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### Struttura Directory Finale

```
apps/stam_client/src/
├── main.rs                    # MODIFICARE: Bevy App invece di tokio loop
├── locale.rs                  # ESISTENTE: mantenere
├── app_paths.rs               # ESISTENTE: mantenere
├── mod_runtime/               # ESISTENTE: mantenere
│   ├── mod.rs
│   └── js_adapter.rs
└── ui/                        # NUOVO: modulo UI Bevy
    ├── mod.rs
    ├── bridge.rs              # UiBridge: canali <-> egui
    ├── systems.rs             # Bevy systems per UI
    └── render.rs              # Rendering widget

apps/shared/stam_mod_runtimes/src/
├── lib.rs                     # ESISTENTE
├── runtime_type.rs            # ESISTENTE
├── logging.rs                 # ESISTENTE
├── api/
│   ├── mod.rs                 # MODIFICARE: export ui, window
│   ├── console.rs             # ESISTENTE
│   ├── process.rs             # ESISTENTE
│   ├── system.rs              # ESISTENTE
│   ├── events.rs              # ESISTENTE
│   ├── locale.rs              # ESISTENTE
│   ├── network.rs             # ESISTENTE
│   ├── ui.rs                  # NUOVO: UiApi, UiCommand, UiEvent
│   └── window.rs              # NUOVO: WindowApi, WindowCommand
└── adapters/js/
    ├── mod.rs                 # ESISTENTE
    ├── runtime.rs             # ESISTENTE
    ├── bindings.rs            # MODIFICARE: aggiungere UiJS, WindowJS
    ├── config.rs              # ESISTENTE
    └── glue/                  # ESISTENTE
```

## API Language-Agnostic (stam_mod_runtimes/src/api/)

### api/ui.rs (NUOVO)

```rust
//! UI API - Language-agnostic UI definitions
//!
//! Queste strutture sono usate da tutti i runtime (JS, Lua, C#, etc.)
//! e comunicate al client tramite canali.

use serde::{Deserialize, Serialize};

/// Comandi UI inviati dai mod al renderer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiCommand {
    RegisterRender { id: String, layout: UiLayout },
    UnregisterRender { id: String },
    UpdateWidget { id: String, state: WidgetState },
    SetTheme { theme: UiTheme },
}

/// Eventi UI inviati dal renderer ai mod
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiEvent {
    ButtonClicked { id: String },
    TextChanged { id: String, value: String },
    CheckboxToggled { id: String, checked: bool },
    DropdownChanged { id: String, index: usize },
    SliderChanged { id: String, value: f32 },
}

/// Layout UI serializzabile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiLayout {
    pub widgets: Vec<Widget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    Label { text: String },
    Button { id: String, text: String },
    ProgressBar { id: String, value: f32, show_percentage: bool },
    TextInput { id: String, value: String, placeholder: Option<String> },
    Checkbox { id: String, label: String, checked: bool },
    Slider { id: String, value: f32, min: f32, max: f32 },
    Spacing { pixels: f32 },
    Separator,
    Horizontal { children: Vec<Widget> },
    Vertical { children: Vec<Widget> },
    Window { id: String, title: String, children: Vec<Widget> },
    Panel { id: String, anchor: Anchor, children: Vec<Widget> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WidgetState {
    pub value: Option<f32>,
    pub text: Option<String>,
    pub checked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiTheme {
    pub background: String,
    pub text: String,
    pub primary: String,
    pub accent: String,
}

/// API UI - usata dai binding di ogni linguaggio
pub struct UiApi {
    command_tx: crossbeam_channel::Sender<UiCommand>,
    event_rx: crossbeam_channel::Receiver<UiEvent>,
}

impl UiApi {
    pub fn new(
        command_tx: crossbeam_channel::Sender<UiCommand>,
        event_rx: crossbeam_channel::Receiver<UiEvent>,
    ) -> Self {
        Self { command_tx, event_rx }
    }

    pub fn register_render(&self, id: &str, layout: UiLayout) -> Result<(), String> {
        self.command_tx.send(UiCommand::RegisterRender {
            id: id.to_string(),
            layout,
        }).map_err(|e| e.to_string())
    }

    pub fn unregister_render(&self, id: &str) -> Result<(), String> {
        self.command_tx.send(UiCommand::UnregisterRender {
            id: id.to_string(),
        }).map_err(|e| e.to_string())
    }

    pub fn update_widget(&self, id: &str, state: WidgetState) -> Result<(), String> {
        self.command_tx.send(UiCommand::UpdateWidget {
            id: id.to_string(),
            state,
        }).map_err(|e| e.to_string())
    }

    pub fn poll_events(&self) -> Vec<UiEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}
```

### api/window.rs (NUOVO)

```rust
//! Window API - Language-agnostic window control

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowCommand {
    SetTitle(String),
    SetSize { width: u32, height: u32 },
    SetFullscreen(bool),
    RequestClose,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowEvent {
    Resized { width: u32, height: u32 },
    Focused(bool),
    CloseRequested,
}

pub struct WindowApi {
    command_tx: crossbeam_channel::Sender<WindowCommand>,
    size: std::sync::Arc<std::sync::RwLock<(u32, u32)>>,
}

impl WindowApi {
    pub fn new(
        command_tx: crossbeam_channel::Sender<WindowCommand>,
        size: std::sync::Arc<std::sync::RwLock<(u32, u32)>>,
    ) -> Self {
        Self { command_tx, size }
    }

    pub fn set_title(&self, title: &str) -> Result<(), String> {
        self.command_tx.send(WindowCommand::SetTitle(title.to_string()))
            .map_err(|e| e.to_string())
    }

    pub fn set_fullscreen(&self, fullscreen: bool) -> Result<(), String> {
        self.command_tx.send(WindowCommand::SetFullscreen(fullscreen))
            .map_err(|e| e.to_string())
    }

    pub fn get_size(&self) -> (u32, u32) {
        *self.size.read().unwrap()
    }
}
```

## Binding JavaScript (adapters/js/bindings.rs)

Aggiungere alle classi esistenti:

```rust
// In bindings.rs, aggiungere:

/// JavaScript UI API class
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct UiJS {
    #[qjs(skip_trace)]
    ui_api: UiApi,
}

#[rquickjs::methods]
impl UiJS {
    #[qjs(rename = "register_render")]
    pub fn register_render(&self, id: String, layout: rquickjs::Value) -> rquickjs::Result<()> {
        // Converti Value JS -> JSON -> UiLayout
        let json = serde_json::to_string(&layout)
            .map_err(|_| rquickjs::Error::Exception)?;
        let layout: UiLayout = serde_json::from_str(&json)
            .map_err(|_| rquickjs::Error::Exception)?;

        self.ui_api.register_render(&id, layout)
            .map_err(|_| rquickjs::Error::Exception)
    }

    #[qjs(rename = "unregister_render")]
    pub fn unregister_render(&self, id: String) -> rquickjs::Result<()> {
        self.ui_api.unregister_render(&id)
            .map_err(|_| rquickjs::Error::Exception)
    }

    #[qjs(rename = "update_widget")]
    pub fn update_widget(&self, id: String, state: rquickjs::Object) -> rquickjs::Result<()> {
        let state = WidgetState {
            value: state.get("value").ok(),
            text: state.get("text").ok(),
            checked: state.get("checked").ok(),
        };
        self.ui_api.update_widget(&id, state)
            .map_err(|_| rquickjs::Error::Exception)
    }

    #[qjs(rename = "poll_events")]
    pub fn poll_events<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Array<'js>> {
        let events = self.ui_api.poll_events();
        let array = Array::new(ctx.clone())?;

        for (i, event) in events.iter().enumerate() {
            let obj = Object::new(ctx.clone())?;
            match event {
                UiEvent::ButtonClicked { id } => {
                    obj.set("type", "ButtonClicked")?;
                    obj.set("id", id.as_str())?;
                }
                UiEvent::TextChanged { id, value } => {
                    obj.set("type", "TextChanged")?;
                    obj.set("id", id.as_str())?;
                    obj.set("value", value.as_str())?;
                }
                // ... altri eventi
            }
            array.set(i, obj)?;
        }

        Ok(array)
    }
}

/// Setup UI API nel contesto JavaScript
pub fn setup_ui_api(ctx: Ctx, ui_api: UiApi) -> Result<(), rquickjs::Error> {
    rquickjs::Class::<UiJS>::define(&ctx.globals())?;
    let ui_obj = rquickjs::Class::<UiJS>::instance(ctx.clone(), UiJS { ui_api })?;
    ctx.globals().set("ui", ui_obj)?;
    Ok(())
}
```

## Client: Integrazione Bevy (stam_client)

### main.rs - Transizione a Bevy

Il main.rs attuale usa `tokio::select!` per gestire eventi. Con Bevy:

```rust
// main.rs - nuova struttura

use bevy::prelude::*;
use bevy_egui::EguiPlugin;

fn main() {
    // Setup tokio runtime per networking (in background thread)
    let tokio_runtime = tokio::runtime::Runtime::new().unwrap();

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Staminal".into(),
                resolution: (1280., 720.).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin)
        .insert_resource(TokioRuntime(tokio_runtime))
        .add_systems(Startup, setup_networking)
        .add_systems(Startup, setup_mod_runtime)
        .add_systems(Update, process_network_events)
        .add_systems(Update, process_ui_commands)
        .add_systems(Update, render_ui)
        .run();
}

#[derive(Resource)]
struct TokioRuntime(tokio::runtime::Runtime);

#[derive(Resource)]
struct UiBridge {
    command_rx: crossbeam_channel::Receiver<UiCommand>,
    event_tx: crossbeam_channel::Sender<UiEvent>,
    active_layouts: HashMap<String, UiLayout>,
    widget_states: HashMap<String, WidgetState>,
}
```

### ui/bridge.rs (NUOVO)

```rust
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, unbounded};
use stam_mod_runtimes::api::ui::{UiCommand, UiEvent, UiLayout, WidgetState};
use std::collections::HashMap;

#[derive(Resource)]
pub struct UiBridge {
    pub command_rx: Receiver<UiCommand>,
    pub event_tx: Sender<UiEvent>,
    pub active_layouts: HashMap<String, UiLayout>,
    pub widget_states: HashMap<String, WidgetState>,
}

impl UiBridge {
    /// Crea UiBridge e ritorna i canali da passare a UiApi
    pub fn new() -> (Self, Sender<UiCommand>, Receiver<UiEvent>) {
        let (cmd_tx, cmd_rx) = unbounded();
        let (evt_tx, evt_rx) = unbounded();

        let bridge = Self {
            command_rx: cmd_rx,
            event_tx: evt_tx,
            active_layouts: HashMap::new(),
            widget_states: HashMap::new(),
        };

        (bridge, cmd_tx, evt_rx)
    }
}
```

### ui/systems.rs (NUOVO)

```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use super::bridge::UiBridge;
use stam_mod_runtimes::api::ui::{UiCommand, UiEvent, Widget};

/// Processa comandi UI dai mod
pub fn process_ui_commands(mut bridge: ResMut<UiBridge>) {
    while let Ok(cmd) = bridge.command_rx.try_recv() {
        match cmd {
            UiCommand::RegisterRender { id, layout } => {
                bridge.active_layouts.insert(id, layout);
            }
            UiCommand::UnregisterRender { id } => {
                bridge.active_layouts.remove(&id);
            }
            UiCommand::UpdateWidget { id, state } => {
                bridge.widget_states.insert(id, state);
            }
            UiCommand::SetTheme { theme } => {
                // TODO: applicare tema a egui
            }
        }
    }
}

/// Renderizza UI usando egui
pub fn render_ui(mut egui_ctx: EguiContexts, mut bridge: ResMut<UiBridge>) {
    let ctx = egui_ctx.ctx_mut();

    // Clona per evitare borrow issues
    let layouts: Vec<_> = bridge.active_layouts.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    for (_id, layout) in layouts {
        for widget in &layout.widgets {
            render_widget(ctx, widget, &bridge.widget_states, &bridge.event_tx);
        }
    }
}

fn render_widget(
    ctx: &egui::Context,
    widget: &Widget,
    states: &HashMap<String, WidgetState>,
    event_tx: &Sender<UiEvent>,
) {
    match widget {
        Widget::Window { id, title, children } => {
            egui::Window::new(title).show(ctx, |ui| {
                for child in children {
                    render_widget_inner(ui, child, states, event_tx);
                }
            });
        }
        Widget::Panel { id, anchor, children } => {
            egui::CentralPanel::default().show(ctx, |ui| {
                for child in children {
                    render_widget_inner(ui, child, states, event_tx);
                }
            });
        }
        _ => {
            // Widget top-level non in container
            egui::CentralPanel::default().show(ctx, |ui| {
                render_widget_inner(ui, widget, states, event_tx);
            });
        }
    }
}

fn render_widget_inner(
    ui: &mut egui::Ui,
    widget: &Widget,
    states: &HashMap<String, WidgetState>,
    event_tx: &Sender<UiEvent>,
) {
    match widget {
        Widget::Label { text } => {
            ui.label(text);
        }
        Widget::Button { id, text } => {
            if ui.button(text).clicked() {
                let _ = event_tx.send(UiEvent::ButtonClicked { id: id.clone() });
            }
        }
        Widget::ProgressBar { id, value, show_percentage } => {
            let current = states.get(id)
                .and_then(|s| s.value)
                .unwrap_or(*value);
            ui.add(egui::ProgressBar::new(current).show_percentage(*show_percentage));
        }
        Widget::Spacing { pixels } => {
            ui.add_space(*pixels);
        }
        Widget::Separator => {
            ui.separator();
        }
        Widget::Horizontal { children } => {
            ui.horizontal(|ui| {
                for child in children {
                    render_widget_inner(ui, child, states, event_tx);
                }
            });
        }
        Widget::Vertical { children } => {
            ui.vertical(|ui| {
                for child in children {
                    render_widget_inner(ui, child, states, event_tx);
                }
            });
        }
        // ... altri widget
        _ => {}
    }
}
```

## Esempio d'uso in JavaScript

```javascript
// mods/mods-manager/client/manager.js

export class Manager {
    constructor() {
        this.downloadProgress = 0;
    }

    async prepare_ui() {
        // Registra il layout UI
        ui.register_render("loading_screen", {
            widgets: [
                {
                    type: "window",
                    id: "loading_window",
                    title: locale.get("loading-title"),
                    children: [
                        { type: "label", text: locale.get("loading-mods") },
                        {
                            type: "progress_bar",
                            id: "download_progress",
                            value: 0,
                            show_percentage: true
                        },
                        { type: "spacing", pixels: 20 },
                        {
                            type: "horizontal",
                            children: [
                                { type: "button", id: "btn_cancel", text: locale.get("cancel") }
                            ]
                        }
                    ]
                }
            ]
        });
    }

    async ensure_mods() {
        // ... durante il download, aggiorna il progresso
        ui.update_widget("download_progress", { value: this.downloadProgress });

        // Gestisci eventi UI
        const events = ui.poll_events();
        for (const event of events) {
            if (event.type === "ButtonClicked" && event.id === "btn_cancel") {
                await this.cancel_download();
            }
        }
    }

    async cleanup_ui() {
        ui.unregister_render("loading_screen");
    }
}
```

## File da Creare/Modificare

### NUOVI FILE

**stam_mod_runtimes:**
- `src/api/ui.rs` - UiApi, UiCommand, UiEvent, Widget, UiLayout
- `src/api/window.rs` - WindowApi, WindowCommand, WindowEvent

**stam_client:**
- `src/ui/mod.rs` - export modulo
- `src/ui/bridge.rs` - UiBridge resource
- `src/ui/systems.rs` - Bevy systems

### FILE DA MODIFICARE

**stam_mod_runtimes:**
- `src/api/mod.rs` - aggiungere `pub mod ui; pub mod window;`
- `src/adapters/js/bindings.rs` - aggiungere UiJS, WindowJS, setup_ui_api
- `Cargo.toml` - aggiungere `crossbeam-channel`, `serde`

**stam_client:**
- `Cargo.toml` - aggiungere bevy, bevy_egui, crossbeam-channel
- `src/main.rs` - riscrivere con Bevy App (mantenendo logica networking esistente)

## Dipendenze da Aggiungere

### stam_client/Cargo.toml
```toml
[dependencies]
bevy = { version = "0.14", default-features = false, features = [
    "bevy_winit",
    "bevy_render",
    "bevy_core_pipeline",
    "bevy_ui",
    "bevy_text",
    "x11",
    "wayland",
] }
bevy_egui = "0.28"
crossbeam-channel = "0.5"
```

### stam_mod_runtimes/Cargo.toml
```toml
[dependencies]
crossbeam-channel = "0.5"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

## Milestones

- [ ] **M1**: Aggiungere UiApi e WindowApi a stam_mod_runtimes/src/api/
- [ ] **M2**: Aggiungere UiJS e WindowJS ai binding JavaScript
- [ ] **M3**: Setup Bevy in stam_client (finestra vuota)
- [ ] **M4**: Creare UiBridge e systems
- [ ] **M5**: Connettere UiApi ai binding JS nel runtime
- [ ] **M6**: Widget base funzionanti (label, button, progress)
- [ ] **M7**: Sistema eventi UI completo
- [ ] **M8**: Integrare con mods-manager per test reale
- [ ] **M9**: Testing cross-platform

## Note per l'Agente

- **NON creare directory `plugins/`** - non esiste nel progetto
- Le API vanno in `stam_mod_runtimes/src/api/` (esistente)
- I binding JS vanno in `stam_mod_runtimes/src/adapters/js/bindings.rs` (esistente)
- Il client ha gia `mod_runtime/` - mantenerlo, integrare con Bevy
- Seguire le Golden Rules in CLAUDE.md
- NON modificare file in `mods/` senza permesso
- Usare `tracing` per logging
- Il networking tokio deve continuare a funzionare (in background thread)
