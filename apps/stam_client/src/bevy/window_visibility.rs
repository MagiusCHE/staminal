//! Window visibility management with Wayland workaround
//!
//! On Wayland, the `visible` property of windows is not supported and setting
//! it to `false` can cause the window to become stuck/unresponsive.
//!
//! This module provides a workaround by:
//! - NEVER setting `window.visible = false` on Wayland
//! - Tracking logical visibility separately in `WindowVisibilityStates`
//! - Minimizing and moving off-screen when "hiding"
//! - Restoring state when "showing"
//! - Intercepting state changes while logically hidden and storing them for later

use bevy::prelude::*;
use bevy::window::WindowMode;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Cached result of Wayland detection
static IS_WAYLAND: OnceLock<bool> = OnceLock::new();

/// Check if we're running on Wayland
pub fn is_wayland() -> bool {
    *IS_WAYLAND.get_or_init(|| {
        // Check WAYLAND_DISPLAY environment variable
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            // Also check XDG_SESSION_TYPE for more accuracy
            if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
                if session_type == "wayland" {
                    tracing::info!("Detected Wayland session (XDG_SESSION_TYPE=wayland)");
                    return true;
                }
            }
            tracing::info!("Detected Wayland session (WAYLAND_DISPLAY set)");
            return true;
        }
        tracing::debug!("Not running on Wayland");
        false
    })
}

/// Stored window state for Wayland workaround
#[derive(Clone, Debug)]
pub struct StoredWindowState {
    /// Original window position
    pub position: WindowPosition,
    /// Original window resolution
    pub resolution: (f32, f32),
    /// Original window mode
    pub mode: WindowMode,
    /// Original skip_taskbar setting
    pub skip_taskbar: bool,
    /// Original title
    pub title: String,
    /// Original resizable flag
    pub resizable: bool,
}

impl StoredWindowState {
    /// Create from a Window
    pub fn from_window(window: &Window) -> Self {
        Self {
            position: window.position.clone(),
            resolution: (window.resolution.width(), window.resolution.height()),
            mode: window.mode.clone(),
            skip_taskbar: window.skip_taskbar,
            title: window.title.clone(),
            resizable: window.resizable,
        }
    }

    /// Apply stored state to a Window
    pub fn apply_to_window(&self, window: &mut Window) {
        window.position = self.position.clone();
        window.resolution.set(self.resolution.0, self.resolution.1);
        window.mode = self.mode.clone();
        window.skip_taskbar = self.skip_taskbar;
        window.title = self.title.clone();
        window.resizable = self.resizable;
    }
}

/// Resource to store window states for the Wayland workaround
#[derive(Resource, Default)]
pub struct WindowVisibilityStates {
    /// Map of Entity -> stored state (only populated on Wayland when window is hidden)
    states: HashMap<Entity, StoredWindowState>,
}

impl WindowVisibilityStates {
    /// Store window state before hiding (Wayland workaround)
    pub fn store_state(&mut self, entity: Entity, window: &Window) {
        let state = StoredWindowState::from_window(window);
        tracing::debug!("Storing window state for {:?}: {:?}", entity, state);
        self.states.insert(entity, state);
    }

    /// Retrieve and remove stored state (Wayland workaround)
    pub fn take_state(&mut self, entity: Entity) -> Option<StoredWindowState> {
        self.states.remove(&entity)
    }

    /// Check if we have stored state for a window (i.e., window is hidden on Wayland)
    pub fn is_hidden_on_wayland(&self, entity: Entity) -> bool {
        self.states.contains_key(&entity)
    }

    /// Get mutable reference to stored state for updating while hidden
    pub fn get_state_mut(&mut self, entity: Entity) -> Option<&mut StoredWindowState> {
        self.states.get_mut(&entity)
    }

    /// Update position in stored state (for hidden windows on Wayland)
    pub fn update_position(&mut self, entity: Entity, position: WindowPosition) {
        if let Some(state) = self.states.get_mut(&entity) {
            tracing::debug!("Updating stored position for hidden window {:?}: {:?}", entity, position);
            state.position = position;
        }
    }

    /// Update resolution in stored state (for hidden windows on Wayland)
    pub fn update_resolution(&mut self, entity: Entity, width: f32, height: f32) {
        if let Some(state) = self.states.get_mut(&entity) {
            tracing::debug!("Updating stored resolution for hidden window {:?}: {}x{}", entity, width, height);
            state.resolution = (width, height);
        }
    }

    /// Update mode in stored state (for hidden windows on Wayland)
    pub fn update_mode(&mut self, entity: Entity, mode: WindowMode) {
        if let Some(state) = self.states.get_mut(&entity) {
            tracing::debug!("Updating stored mode for hidden window {:?}: {:?}", entity, mode);
            state.mode = mode;
        }
    }

    /// Update skip_taskbar in stored state (for hidden windows on Wayland)
    pub fn update_skip_taskbar(&mut self, entity: Entity, skip_taskbar: bool) {
        if let Some(state) = self.states.get_mut(&entity) {
            tracing::debug!("Updating stored skip_taskbar for hidden window {:?}: {}", entity, skip_taskbar);
            state.skip_taskbar = skip_taskbar;
        }
    }

    /// Update title in stored state (for hidden windows on Wayland)
    pub fn update_title(&mut self, entity: Entity, title: String) {
        if let Some(state) = self.states.get_mut(&entity) {
            tracing::debug!("Updating stored title for hidden window {:?}: {}", entity, title);
            state.title = title;
        }
    }

    /// Update resizable in stored state (for hidden windows on Wayland)
    pub fn update_resizable(&mut self, entity: Entity, resizable: bool) {
        if let Some(state) = self.states.get_mut(&entity) {
            tracing::debug!("Updating stored resizable for hidden window {:?}: {}", entity, resizable);
            state.resizable = resizable;
        }
    }
}

/// Ensure window visibility with Wayland workaround
///
/// On Wayland, we NEVER set `window.visible = false` as it can cause the window
/// to become stuck. Instead, we track logical visibility in `WindowVisibilityStates`
/// and simulate hiding by minimizing + moving off-screen.
///
/// # Arguments
/// * `window` - Mutable reference to the Window component
/// * `entity` - Entity of the window
/// * `visible` - Target visibility state
/// * `force` - If true, apply workaround even if visibility seems unchanged (used at creation)
/// * `states` - Mutable reference to WindowVisibilityStates resource
pub fn ensure_window_visibility(
    window: &mut Window,
    entity: Entity,
    visible: bool,
    force: bool,
    states: &mut WindowVisibilityStates,
) {
    // On non-Wayland, just set visible directly
    if !is_wayland() {
        let current_visible = window.visible;
        if current_visible != visible || force {
            tracing::debug!("Setting window {:?} visible={} (non-Wayland)", entity, visible);
            window.visible = visible;
        }
        return;
    }

    // Wayland workaround - track logical visibility separately
    // NEVER set window.visible = false on Wayland!
    let is_currently_hidden = states.is_hidden_on_wayland(entity);

    if visible && !is_currently_hidden && !force {
        // Already visible, nothing to do
        return;
    }

    if !visible && is_currently_hidden && !force {
        // Already hidden, nothing to do
        return;
    }

    if visible {
        // Showing window: restore stored state
        tracing::info!("Wayland workaround: showing window {:?}", entity);

        if let Some(stored) = states.take_state(entity) {
            tracing::debug!("Restoring window state: {:?}", stored);
            stored.apply_to_window(window);
        }

        // Keep visible = true (it should already be true on Wayland)
        window.visible = true;
    } else {
        // Hiding window: store state and apply workaround
        tracing::info!("Wayland workaround: hiding window {:?}", entity);

        // Store current state before modifying
        states.store_state(entity, window);

        // Apply hiding workaround:
        // - Move off-screen
        // - Set minimum size (1x1 pixel)
        // - Hide from taskbar
        // - DO NOT minimize (causes window to become unresponsive on Wayland)
        // - DO NOT set visible = false (causes window to get stuck on Wayland)
        // Use large positive coordinates - negative values may be ignored on Wayland
        window.position = WindowPosition::At(IVec2::new(99999, 99999));
        window.resolution.set(1.0, 1.0);
        window.skip_taskbar = true;
        // window.visible remains true on Wayland!
    }
}

/// Apply initial visibility workaround for newly created windows
///
/// Called when a window is created with visible=false on Wayland.
/// On Wayland, we set visible=true but apply the hiding workaround
/// (minimize + off-screen) to simulate hidden state.
pub fn apply_initial_hidden_state(
    window: &mut Window,
    entity: Entity,
    states: &mut WindowVisibilityStates,
) {
    if !is_wayland() {
        // Non-Wayland: visible=false should work, nothing to do
        return;
    }

    // On Wayland, we need to apply the hidden state workaround
    // The window was created with visible=false, but we must set it to true
    // and simulate hiding instead
    if !window.visible {
        tracing::info!("Wayland workaround: applying initial hidden state for window {:?}", entity);

        // Store the intended initial state (with the original values before we modify them)
        states.store_state(entity, window);

        // Apply hiding workaround:
        // - Set visible = true (NEVER false on Wayland!)
        // - Move off-screen
        // - Set minimum size (1x1 pixel)
        // - Hide from taskbar
        // - DO NOT minimize (causes window to become unresponsive)
        window.visible = true; // Must be true on Wayland to avoid stuck window
        // Use large positive coordinates - negative values may be ignored on Wayland
        window.position = WindowPosition::At(IVec2::new(99999, 99999));
        window.resolution.set(1.0, 1.0);
        window.skip_taskbar = true;
    }
}

/// Check if a window property change should be stored instead of applied
/// Returns true if on Wayland and window is hidden (state should be stored, not applied)
pub fn should_store_instead_of_apply(entity: Entity, states: &WindowVisibilityStates) -> bool {
    is_wayland() && states.is_hidden_on_wayland(entity)
}
