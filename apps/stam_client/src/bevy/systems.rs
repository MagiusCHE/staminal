//! Bevy ECS systems for UI rendering and window management
//!
//! This module contains Bevy systems that:
//! - Process UI commands from scripts
//! - Render UI layouts using egui
//! - Handle UI events and send them back to scripts
//! - Manage window state

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowPosition};
use bevy_egui::{EguiContexts, egui};

use stam_mod_runtimes::api::{Anchor, UiCommand, UiEvent, UiLayout, Widget, WidgetState, WindowCommand, WindowPositionMode, MAIN_WINDOW_ID};

use super::ui_bridge::UiBridge;
use super::window_visibility::{WindowVisibilityStates, ensure_window_visibility, apply_initial_hidden_state, should_store_instead_of_apply, is_wayland};

/// Resource that holds all registered UI layouts from scripts
#[derive(Resource, Default)]
pub struct UiLayouts {
    /// Map of layout ID -> UiLayout
    pub layouts: std::collections::HashMap<String, UiLayout>,
    /// Widget states (for dynamic updates)
    pub widget_states: std::collections::HashMap<String, WidgetState>,
}

/// Component to track script window ID on Bevy Window entities
#[derive(Component)]
pub struct ScriptWindowId(pub u32);

/// Marker component for windows that need initial hidden state applied (Wayland workaround)
#[derive(Component)]
pub struct NeedsInitialHiddenState;

/// Resource to map script window IDs to Bevy entities
#[derive(Resource, Default)]
pub struct WindowRegistry {
    /// Map of script window ID -> Bevy entity
    pub windows: std::collections::HashMap<u32, Entity>,
}

/// System to process UI commands from scripts
pub fn process_ui_commands(
    bridge: Res<UiBridge>,
    mut layouts: ResMut<UiLayouts>,
) {
    for cmd in bridge.poll_ui_commands() {
        match cmd {
            UiCommand::RegisterRender { id, layout } => {
                tracing::debug!("Registering UI layout: {}", id);
                layouts.layouts.insert(id, layout);
            }
            UiCommand::UnregisterRender { id } => {
                tracing::debug!("Unregistering UI layout: {}", id);
                layouts.layouts.remove(&id);
            }
            UiCommand::UpdateWidget { id, state } => {
                tracing::trace!("Updating widget state: {}", id);
                layouts.widget_states.insert(id, state);
            }
            UiCommand::SetTheme { theme: _ } => {
                // TODO: Implement theme switching
                tracing::debug!("Theme switching not yet implemented");
            }
        }
    }
}

/// System to process window commands from scripts
pub fn process_window_commands(
    bridge: Res<UiBridge>,
    mut primary_window_query: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut other_windows_query: Query<(Entity, &mut Window, &ScriptWindowId), Without<PrimaryWindow>>,
    mut commands: Commands,
    mut registry: ResMut<WindowRegistry>,
    mut visibility_states: ResMut<WindowVisibilityStates>,
) {
    for cmd in bridge.poll_window_commands() {
        match cmd {
            WindowCommand::Create { id, title, width, height, resizable } => {
                // Create always creates a NEW window (id > 0), hidden by default
                tracing::info!("Creating new window #{}: {} ({}x{}, resizable={}) - hidden by default", id, title, width, height, resizable);
                let entity = commands.spawn((
                    Window {
                        title,
                        resolution: bevy::window::WindowResolution::new(width, height),
                        resizable,
                        visible: false, // Windows are created hidden, use .show() to make visible
                        ..default()
                    },
                    ScriptWindowId(id),
                    // Mark for initial hidden state application (handled by apply_new_windows_hidden_state)
                    NeedsInitialHiddenState,
                )).id();
                registry.windows.insert(id, entity);
            }
            WindowCommand::SetTitle { id, title } => {
                if id == MAIN_WINDOW_ID {
                    if let Ok((entity, mut window)) = primary_window_query.single_mut() {
                        if should_store_instead_of_apply(entity, &visibility_states) {
                            tracing::debug!("Storing main window title (hidden on Wayland): {}", title);
                            visibility_states.update_title(entity, title);
                        } else {
                            tracing::debug!("Setting main window title: {}", title);
                            window.title = title;
                        }
                    }
                } else if let Some(&entity) = registry.windows.get(&id) {
                    if should_store_instead_of_apply(entity, &visibility_states) {
                        tracing::debug!("Storing window #{} title (hidden on Wayland): {}", id, title);
                        visibility_states.update_title(entity, title);
                    } else {
                        for (e, mut window, _) in other_windows_query.iter_mut() {
                            if e == entity {
                                tracing::debug!("Setting window #{} title: {}", id, title);
                                window.title = title;
                                break;
                            }
                        }
                    }
                }
            }
            WindowCommand::SetSize { id, width, height } => {
                if id == MAIN_WINDOW_ID {
                    if let Ok((entity, mut window)) = primary_window_query.single_mut() {
                        if should_store_instead_of_apply(entity, &visibility_states) {
                            tracing::debug!("Storing main window size (hidden on Wayland): {}x{}", width, height);
                            visibility_states.update_resolution(entity, width as f32, height as f32);
                        } else {
                            tracing::debug!("Setting main window size: {}x{}", width, height);
                            window.resolution.set(width as f32, height as f32);
                        }
                    }
                } else if let Some(&entity) = registry.windows.get(&id) {
                    if should_store_instead_of_apply(entity, &visibility_states) {
                        tracing::debug!("Storing window #{} size (hidden on Wayland): {}x{}", id, width, height);
                        visibility_states.update_resolution(entity, width as f32, height as f32);
                    } else {
                        for (e, mut window, _) in other_windows_query.iter_mut() {
                            if e == entity {
                                tracing::debug!("Setting window #{} size: {}x{}", id, width, height);
                                window.resolution.set(width as f32, height as f32);
                                break;
                            }
                        }
                    }
                }
            }
            WindowCommand::SetFullscreen { id, fullscreen } => {
                let mode = if fullscreen {
                    bevy::window::WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                } else {
                    bevy::window::WindowMode::Windowed
                };
                if id == MAIN_WINDOW_ID {
                    if let Ok((entity, mut window)) = primary_window_query.single_mut() {
                        if should_store_instead_of_apply(entity, &visibility_states) {
                            tracing::debug!("Storing main window fullscreen (hidden on Wayland): {}", fullscreen);
                            visibility_states.update_mode(entity, mode);
                        } else {
                            tracing::debug!("Setting main window fullscreen: {}", fullscreen);
                            window.mode = mode;
                        }
                    }
                } else if let Some(&entity) = registry.windows.get(&id) {
                    if should_store_instead_of_apply(entity, &visibility_states) {
                        tracing::debug!("Storing window #{} fullscreen (hidden on Wayland): {}", id, fullscreen);
                        visibility_states.update_mode(entity, mode);
                    } else {
                        for (e, mut window, _) in other_windows_query.iter_mut() {
                            if e == entity {
                                tracing::debug!("Setting window #{} fullscreen: {}", id, fullscreen);
                                window.mode = mode;
                                break;
                            }
                        }
                    }
                }
            }
            WindowCommand::SetResizable { id, resizable } => {
                if id == MAIN_WINDOW_ID {
                    if let Ok((entity, mut window)) = primary_window_query.single_mut() {
                        if should_store_instead_of_apply(entity, &visibility_states) {
                            tracing::debug!("Storing main window resizable (hidden on Wayland): {}", resizable);
                            visibility_states.update_resizable(entity, resizable);
                        } else {
                            tracing::debug!("Setting main window resizable: {}", resizable);
                            window.resizable = resizable;
                        }
                    }
                } else if let Some(&entity) = registry.windows.get(&id) {
                    if should_store_instead_of_apply(entity, &visibility_states) {
                        tracing::debug!("Storing window #{} resizable (hidden on Wayland): {}", id, resizable);
                        visibility_states.update_resizable(entity, resizable);
                    } else {
                        for (e, mut window, _) in other_windows_query.iter_mut() {
                            if e == entity {
                                tracing::debug!("Setting window #{} resizable: {}", id, resizable);
                                window.resizable = resizable;
                                break;
                            }
                        }
                    }
                }
            }
            WindowCommand::SetVisible { id, visible } => {
                if id == MAIN_WINDOW_ID {
                    if let Ok((entity, mut window)) = primary_window_query.single_mut() {
                        tracing::info!("Setting main window visible: {}", visible);
                        ensure_window_visibility(&mut window, entity, visible, false, &mut visibility_states);
                    }
                } else if let Some(&entity) = registry.windows.get(&id) {
                    for (e, mut window, _) in other_windows_query.iter_mut() {
                        if e == entity {
                            tracing::info!("Setting window #{} visible: {}", id, visible);
                            ensure_window_visibility(&mut window, e, visible, false, &mut visibility_states);
                            break;
                        }
                    }
                }
            }
            WindowCommand::SetPositionMode { id, mode } => {
                // Convert WindowPositionMode to Bevy's WindowPosition
                let bevy_position = match mode {
                    WindowPositionMode::Automatic => WindowPosition::Automatic,
                    WindowPositionMode::Centered => WindowPosition::Centered(MonitorSelection::Current),
                    WindowPositionMode::Manual => {
                        // Manual mode doesn't change the position, just sets it to current position
                        // This is used before calling set_position() to enable manual positioning
                        tracing::debug!("Window #{} set to Manual position mode (position unchanged)", id);
                        // Keep current position - we don't change it here
                        // A separate SetPosition command would be needed to actually move the window
                        return;
                    }
                };

                if id == MAIN_WINDOW_ID {
                    if let Ok((entity, mut window)) = primary_window_query.single_mut() {
                        if should_store_instead_of_apply(entity, &visibility_states) {
                            tracing::debug!("Storing main window position mode (hidden on Wayland): {:?}", mode);
                            visibility_states.update_position(entity, bevy_position);
                        } else {
                            tracing::debug!("Setting main window position mode: {:?}", mode);
                            window.position = bevy_position;
                        }
                    }
                } else if let Some(&entity) = registry.windows.get(&id) {
                    if should_store_instead_of_apply(entity, &visibility_states) {
                        tracing::debug!("Storing window #{} position mode (hidden on Wayland): {:?}", id, mode);
                        visibility_states.update_position(entity, bevy_position);
                    } else {
                        for (e, mut window, _) in other_windows_query.iter_mut() {
                            if e == entity {
                                tracing::debug!("Setting window #{} position mode: {:?}", id, mode);
                                window.position = bevy_position;
                                break;
                            }
                        }
                    }
                }
            }
            WindowCommand::RequestClose { id } => {
                tracing::info!("Window #{} close requested by script", id);
                // TODO: Handle window close
            }
        }
    }
}

/// System to update window size in the bridge when window is resized
pub fn sync_window_size(
    bridge: Res<UiBridge>,
    window_query: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
) {
    if let Ok(window) = window_query.single() {
        let width = window.resolution.width() as u32;
        let height = window.resolution.height() as u32;
        bridge.update_window_size(width, height);
    }
}

/// System to check if shutdown has been requested and exit the app
pub fn check_shutdown(
    bridge: Res<UiBridge>,
    mut exit: EventWriter<AppExit>,
) {
    if bridge.should_shutdown() {
        tracing::info!("Shutdown requested, exiting Bevy...");
        exit.write(AppExit::Success);
    }
}

/// System to render UI layouts using egui
pub fn render_ui_layouts(
    mut contexts: EguiContexts,
    layouts: Res<UiLayouts>,
    bridge: Res<UiBridge>,
) {
    // Get the primary window's egui context
    let Ok(ctx) = contexts.ctx_mut() else {
        return; // No context available yet
    };

    // Clone layouts data to avoid borrowing issues
    let layouts_clone: Vec<(String, UiLayout)> = layouts.layouts
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let widget_states_clone = layouts.widget_states.clone();

    // Render each registered layout
    for (layout_id, layout) in &layouts_clone {
        render_layout(ctx, layout_id, &layout, &widget_states_clone, &bridge);
    }
}

/// Render a single UI layout
fn render_layout(
    ctx: &egui::Context,
    layout_id: &str,
    layout: &UiLayout,
    widget_states: &std::collections::HashMap<String, WidgetState>,
    bridge: &UiBridge,
) {
    // Render each widget in the layout
    for widget in &layout.widgets {
        render_widget(ctx, layout_id, widget, widget_states, bridge);
    }
}

/// Render a single widget (top-level, creates its own area/window)
fn render_widget(
    ctx: &egui::Context,
    layout_id: &str,
    widget: &Widget,
    widget_states: &std::collections::HashMap<String, WidgetState>,
    bridge: &UiBridge,
) {
    match widget {
        Widget::Label { text } => {
            egui::Area::new(egui::Id::new(format!("{}_{}", layout_id, text)))
                .show(ctx, |ui| {
                    ui.label(text);
                });
        }

        Widget::Button { id, text } => {
            egui::Area::new(egui::Id::new(format!("{}_{}", layout_id, id)))
                .show(ctx, |ui| {
                    if ui.button(text).clicked() {
                        bridge.send_ui_event(UiEvent::ButtonClicked { id: id.clone() });
                    }
                });
        }

        Widget::ProgressBar { id, value, show_percentage } => {
            // Check for updated state (WidgetState.value is Option<f32>)
            let current_value = widget_states
                .get(id)
                .and_then(|s| s.value)
                .unwrap_or(*value);

            egui::Area::new(egui::Id::new(format!("{}_{}", layout_id, id)))
                .show(ctx, |ui| {
                    let progress_bar = egui::ProgressBar::new(current_value);
                    let progress_bar = if *show_percentage {
                        progress_bar.show_percentage()
                    } else {
                        progress_bar
                    };
                    ui.add(progress_bar);
                });
        }

        Widget::TextInput { id, value, placeholder } => {
            // WidgetState.text is Option<String>
            let current_value = widget_states
                .get(id)
                .and_then(|s| s.text.clone())
                .unwrap_or_else(|| value.clone());

            egui::Area::new(egui::Id::new(format!("{}_{}", layout_id, id)))
                .show(ctx, |ui| {
                    let mut text = current_value;
                    let text_edit = egui::TextEdit::singleline(&mut text);
                    let text_edit = if let Some(ph) = placeholder {
                        text_edit.hint_text(ph)
                    } else {
                        text_edit
                    };

                    if ui.add(text_edit).changed() {
                        bridge.send_ui_event(UiEvent::TextChanged {
                            id: id.clone(),
                            value: text,
                        });
                    }
                });
        }

        Widget::Checkbox { id, label, checked } => {
            // WidgetState.checked is Option<bool>
            let current_checked = widget_states
                .get(id)
                .and_then(|s| s.checked)
                .unwrap_or(*checked);

            egui::Area::new(egui::Id::new(format!("{}_{}", layout_id, id)))
                .show(ctx, |ui| {
                    let mut is_checked = current_checked;
                    if ui.checkbox(&mut is_checked, label).changed() {
                        bridge.send_ui_event(UiEvent::CheckboxToggled {
                            id: id.clone(),
                            checked: is_checked,
                        });
                    }
                });
        }

        Widget::Slider { id, value, min, max } => {
            // WidgetState.value is Option<f32>
            let current_value = widget_states
                .get(id)
                .and_then(|s| s.value)
                .unwrap_or(*value);

            egui::Area::new(egui::Id::new(format!("{}_{}", layout_id, id)))
                .show(ctx, |ui| {
                    let mut v = current_value;
                    if ui.add(egui::Slider::new(&mut v, *min..=*max)).changed() {
                        bridge.send_ui_event(UiEvent::SliderChanged {
                            id: id.clone(),
                            value: v,
                        });
                    }
                });
        }

        Widget::Spacing { pixels } => {
            egui::Area::new(egui::Id::new(format!("{}_spacing", layout_id)))
                .show(ctx, |ui| {
                    ui.add_space(*pixels);
                });
        }

        Widget::Separator => {
            egui::Area::new(egui::Id::new(format!("{}_separator", layout_id)))
                .show(ctx, |ui| {
                    ui.separator();
                });
        }

        Widget::Horizontal { children } => {
            egui::Area::new(egui::Id::new(format!("{}_horizontal", layout_id)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for child in children {
                            render_widget_inline(ui, layout_id, child, widget_states, bridge);
                        }
                    });
                });
        }

        Widget::Vertical { children } => {
            egui::Area::new(egui::Id::new(format!("{}_vertical", layout_id)))
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        for child in children {
                            render_widget_inline(ui, layout_id, child, widget_states, bridge);
                        }
                    });
                });
        }

        Widget::Window { id, title, children } => {
            egui::Window::new(title)
                .id(egui::Id::new(id))
                .show(ctx, |ui| {
                    for child in children {
                        render_widget_inline(ui, layout_id, child, widget_states, bridge);
                    }
                });
        }

        Widget::Panel { id, anchor, children } => {
            let panel_id = id.clone();
            match anchor {
                Anchor::TopLeft | Anchor::TopRight => {
                    egui::TopBottomPanel::top(panel_id).show(ctx, |ui| {
                        for child in children {
                            render_widget_inline(ui, layout_id, child, widget_states, bridge);
                        }
                    });
                }
                Anchor::BottomLeft | Anchor::BottomRight => {
                    egui::TopBottomPanel::bottom(panel_id).show(ctx, |ui| {
                        for child in children {
                            render_widget_inline(ui, layout_id, child, widget_states, bridge);
                        }
                    });
                }
                Anchor::Center => {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        for child in children {
                            render_widget_inline(ui, layout_id, child, widget_states, bridge);
                        }
                    });
                }
            }
        }
    }
}

/// Render a widget inline (within a parent container)
fn render_widget_inline(
    ui: &mut egui::Ui,
    layout_id: &str,
    widget: &Widget,
    widget_states: &std::collections::HashMap<String, WidgetState>,
    bridge: &UiBridge,
) {
    match widget {
        Widget::Label { text } => {
            ui.label(text);
        }

        Widget::Button { id, text } => {
            if ui.button(text).clicked() {
                bridge.send_ui_event(UiEvent::ButtonClicked { id: id.clone() });
            }
        }

        Widget::ProgressBar { id, value, show_percentage } => {
            let current_value = widget_states
                .get(id)
                .and_then(|s| s.value)
                .unwrap_or(*value);

            let progress_bar = egui::ProgressBar::new(current_value);
            let progress_bar = if *show_percentage {
                progress_bar.show_percentage()
            } else {
                progress_bar
            };
            ui.add(progress_bar);
        }

        Widget::TextInput { id, value, placeholder } => {
            let current_value = widget_states
                .get(id)
                .and_then(|s| s.text.clone())
                .unwrap_or_else(|| value.clone());

            let mut text = current_value;
            let text_edit = egui::TextEdit::singleline(&mut text);
            let text_edit = if let Some(ph) = placeholder {
                text_edit.hint_text(ph)
            } else {
                text_edit
            };

            if ui.add(text_edit).changed() {
                bridge.send_ui_event(UiEvent::TextChanged {
                    id: id.clone(),
                    value: text,
                });
            }
        }

        Widget::Checkbox { id, label, checked } => {
            let current_checked = widget_states
                .get(id)
                .and_then(|s| s.checked)
                .unwrap_or(*checked);

            let mut is_checked = current_checked;
            if ui.checkbox(&mut is_checked, label).changed() {
                bridge.send_ui_event(UiEvent::CheckboxToggled {
                    id: id.clone(),
                    checked: is_checked,
                });
            }
        }

        Widget::Slider { id, value, min, max } => {
            let current_value = widget_states
                .get(id)
                .and_then(|s| s.value)
                .unwrap_or(*value);

            let mut v = current_value;
            if ui.add(egui::Slider::new(&mut v, *min..=*max)).changed() {
                bridge.send_ui_event(UiEvent::SliderChanged {
                    id: id.clone(),
                    value: v,
                });
            }
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
                    render_widget_inline(ui, layout_id, child, widget_states, bridge);
                }
            });
        }

        Widget::Vertical { children } => {
            ui.vertical(|ui| {
                for child in children {
                    render_widget_inline(ui, layout_id, child, widget_states, bridge);
                }
            });
        }

        Widget::Window { id, title, children } => {
            // Nested windows rendered as groups
            ui.group(|ui| {
                ui.label(format!("[{}] {}", id, title));
                for child in children {
                    render_widget_inline(ui, layout_id, child, widget_states, bridge);
                }
            });
        }

        Widget::Panel { id: _, anchor: _, children } => {
            // Nested panels rendered as groups
            ui.group(|ui| {
                for child in children {
                    render_widget_inline(ui, layout_id, child, widget_states, bridge);
                }
            });
        }
    }
}

/// Startup system to apply initial hidden state to the primary window (Wayland workaround)
///
/// On Wayland, windows are created with visible=true (to avoid stuck windows),
/// so we need to apply the hiding workaround here.
/// On non-Wayland, windows are created with visible=false, so we only apply
/// the workaround if visible is already false.
pub fn apply_initial_hidden_state_system(
    mut window_query: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut visibility_states: ResMut<WindowVisibilityStates>,
) {
    if let Ok((entity, mut window)) = window_query.single_mut() {
        if is_wayland() {
            // On Wayland: window was created with visible=true, apply hiding workaround
            tracing::info!("Applying initial hidden state to primary window (Wayland)");
            // Store the current state and apply workaround
            visibility_states.store_state(entity, &window);
            // Move off-screen, set minimum size (1x1 pixel) and hide from taskbar
            // IMPORTANT: Use WindowPosition::At (NOT Centered!) - Centered ignores position changes
            // DO NOT minimize - causes window to become unresponsive on Wayland
            // window.visible stays true on Wayland!
            window.position = WindowPosition::At(IVec2::new(99999, 99999));
            window.resolution.set(1.0, 1.0);
            window.skip_taskbar = true;
        }
    }
}

/// System to apply initial hidden state to newly created windows (Wayland workaround)
/// This runs in Update and processes windows with the NeedsInitialHiddenState marker
///
/// On Wayland, all windows must be created with visible=true, so we always apply
/// the hiding workaround.
/// On non-Wayland, windows can be created with visible=false directly.
pub fn apply_new_windows_hidden_state(
    mut commands: Commands,
    mut window_query: Query<(Entity, &mut Window), With<NeedsInitialHiddenState>>,
    mut visibility_states: ResMut<WindowVisibilityStates>,
) {
    for (entity, mut window) in window_query.iter_mut() {
        if is_wayland() {
            // On Wayland: apply hiding workaround regardless of visible state
            tracing::info!("Applying initial hidden state to new window {:?} (Wayland)", entity);
            visibility_states.store_state(entity, &window);
            window.visible = true; // Ensure visible=true on Wayland
            // Move off-screen, set minimum size (1x1 pixel) and hide from taskbar
            // IMPORTANT: Use WindowPosition::At (NOT Centered!) - Centered ignores position changes
            // DO NOT minimize - causes window to become unresponsive on Wayland
            window.position = WindowPosition::At(IVec2::new(99999, 99999));
            window.resolution.set(1.0, 1.0);
            window.skip_taskbar = true;
        }
        // Remove the marker component
        commands.entity(entity).remove::<NeedsInitialHiddenState>();
    }
}
