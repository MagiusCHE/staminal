//! UI API - Language-agnostic UI definitions
//!
//! Queste strutture sono usate da tutti i runtime (JS, Lua, C#, etc.)
//! e comunicate al client tramite canali.
//!
//! # Architecture
//!
//! - **UiCommand**: Comandi inviati dai mod al renderer (register, unregister, update)
//! - **UiEvent**: Eventi inviati dal renderer ai mod (click, change, etc.)
//! - **UiApi**: API usata dai binding di ogni linguaggio per comunicare con il renderer

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

/// Comandi UI inviati dai mod al renderer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiCommand {
    /// Registra un layout UI da renderizzare
    RegisterRender { id: String, layout: UiLayout },
    /// Rimuovi un layout UI
    UnregisterRender { id: String },
    /// Aggiorna lo stato di un widget specifico
    UpdateWidget { id: String, state: WidgetState },
    /// Imposta il tema globale
    SetTheme { theme: UiTheme },
}

/// Eventi UI inviati dal renderer ai mod
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiEvent {
    /// Un bottone e stato cliccato
    ButtonClicked { id: String },
    /// Un input di testo e cambiato
    TextChanged { id: String, value: String },
    /// Un checkbox e stato toggleto
    CheckboxToggled { id: String, checked: bool },
    /// Una dropdown ha cambiato selezione
    DropdownChanged { id: String, index: usize },
    /// Uno slider e stato mosso
    SliderChanged { id: String, value: f32 },
}

/// Layout UI serializzabile (passato dai mod)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiLayout {
    pub widgets: Vec<Widget>,
}

/// Widget UI - definisce i tipi di componenti disponibili
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    /// Testo statico
    Label { text: String },
    /// Bottone cliccabile
    Button { id: String, text: String },
    /// Barra di progresso
    ProgressBar {
        id: String,
        value: f32,
        #[serde(default)]
        show_percentage: bool,
    },
    /// Input di testo
    TextInput {
        id: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: Option<String>,
    },
    /// Checkbox
    Checkbox {
        id: String,
        label: String,
        #[serde(default)]
        checked: bool,
    },
    /// Slider numerico
    Slider {
        id: String,
        value: f32,
        #[serde(default)]
        min: f32,
        #[serde(default = "default_slider_max")]
        max: f32,
    },
    /// Spaziatura verticale
    Spacing { pixels: f32 },
    /// Linea separatrice
    Separator,
    /// Layout orizzontale
    Horizontal { children: Vec<Widget> },
    /// Layout verticale
    Vertical { children: Vec<Widget> },
    /// Finestra con titolo
    Window {
        id: String,
        title: String,
        children: Vec<Widget>,
    },
    /// Pannello ancorato
    Panel {
        id: String,
        anchor: Anchor,
        children: Vec<Widget>,
    },
}

fn default_slider_max() -> f32 {
    1.0
}

/// Posizione di ancoraggio per Panel
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Stato aggiornabile di un widget
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WidgetState {
    /// Valore numerico (per ProgressBar, Slider)
    #[serde(default)]
    pub value: Option<f32>,
    /// Valore testuale (per TextInput, Label)
    #[serde(default)]
    pub text: Option<String>,
    /// Stato checked (per Checkbox)
    #[serde(default)]
    pub checked: Option<bool>,
}

/// Tema UI globale
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiTheme {
    pub background: String,
    pub text: String,
    pub primary: String,
    pub accent: String,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            background: "#1a1a2e".to_string(),
            text: "#eaeaea".to_string(),
            primary: "#0f3460".to_string(),
            accent: "#e94560".to_string(),
        }
    }
}

/// API UI - usata dai binding di ogni linguaggio
///
/// Questa struct viene passata ai runtime di scripting e permette di:
/// - Registrare layout UI da renderizzare
/// - Aggiornare lo stato dei widget
/// - Ricevere eventi UI (click, input changes, etc.)
#[derive(Clone)]
pub struct UiApi {
    command_tx: Sender<UiCommand>,
    event_rx: Receiver<UiEvent>,
}

impl UiApi {
    /// Crea una nuova UiApi con i canali specificati
    pub fn new(command_tx: Sender<UiCommand>, event_rx: Receiver<UiEvent>) -> Self {
        Self {
            command_tx,
            event_rx,
        }
    }

    /// Registra un layout UI da renderizzare
    ///
    /// Il layout verra renderizzato ogni frame finche non viene rimosso con `unregister_render`.
    pub fn register_render(&self, id: &str, layout: UiLayout) -> Result<(), String> {
        self.command_tx
            .send(UiCommand::RegisterRender {
                id: id.to_string(),
                layout,
            })
            .map_err(|e| e.to_string())
    }

    /// Rimuove un layout UI precedentemente registrato
    pub fn unregister_render(&self, id: &str) -> Result<(), String> {
        self.command_tx
            .send(UiCommand::UnregisterRender {
                id: id.to_string(),
            })
            .map_err(|e| e.to_string())
    }

    /// Aggiorna lo stato di un widget specifico
    ///
    /// Utile per aggiornare il valore di una progress bar, il testo di un input, etc.
    pub fn update_widget(&self, id: &str, state: WidgetState) -> Result<(), String> {
        self.command_tx
            .send(UiCommand::UpdateWidget {
                id: id.to_string(),
                state,
            })
            .map_err(|e| e.to_string())
    }

    /// Imposta il tema UI globale
    pub fn set_theme(&self, theme: UiTheme) -> Result<(), String> {
        self.command_tx
            .send(UiCommand::SetTheme { theme })
            .map_err(|e| e.to_string())
    }

    /// Legge tutti gli eventi UI pendenti
    ///
    /// Chiamare questa funzione regolarmente per gestire click, input changes, etc.
    pub fn poll_events(&self) -> Vec<UiEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_serialization() {
        let layout = UiLayout {
            widgets: vec![
                Widget::Label {
                    text: "Hello".to_string(),
                },
                Widget::Button {
                    id: "btn1".to_string(),
                    text: "Click me".to_string(),
                },
                Widget::ProgressBar {
                    id: "progress".to_string(),
                    value: 0.5,
                    show_percentage: true,
                },
            ],
        };

        let json = serde_json::to_string(&layout).unwrap();
        let parsed: UiLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.widgets.len(), 3);
    }

    #[test]
    fn test_widget_deserialization_from_js_format() {
        let json = r#"{
            "widgets": [
                { "type": "label", "text": "Loading..." },
                { "type": "button", "id": "cancel", "text": "Cancel" },
                { "type": "progress_bar", "id": "prog", "value": 0.75, "show_percentage": true }
            ]
        }"#;

        let layout: UiLayout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.widgets.len(), 3);
    }
}
