//! Input State Structures
//!
//! Pre-allocated structures for efficient input state tracking during frame updates.

/// Key code identifier (matches web KeyboardEvent.code)
pub type KeyCode = String;

/// Pre-allocated snapshot of input state for the current frame
///
/// This structure is allocated once and reused every frame to avoid
/// allocations in the hot path.
#[derive(Clone, Debug)]
pub struct FrameSnapshot {
    /// Time since last frame in seconds
    pub delta: f64,
    /// Frame number (monotonically increasing)
    pub frame_number: u64,
    /// Active window ID
    pub window_id: u64,
    /// Mouse position in window coordinates
    pub mouse_x: f32,
    /// Mouse position Y
    pub mouse_y: f32,
    /// Mouse button states
    pub mouse_buttons: MouseButtonState,
    /// Currently pressed keyboard keys
    /// Vec is reused - cleared and refilled each frame
    pub pressed_keys: Vec<KeyCode>,
    /// Gamepad states (up to 4 gamepads)
    pub gamepads: [GamepadState; 4],
    /// Number of connected gamepads
    pub gamepad_count: u8,
}

impl Default for FrameSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameSnapshot {
    /// Create a new snapshot with pre-allocated capacity
    pub fn new() -> Self {
        Self {
            delta: 0.0,
            frame_number: 0,
            window_id: 0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            mouse_buttons: MouseButtonState::default(),
            pressed_keys: Vec::with_capacity(16), // Pre-allocate for typical use
            gamepads: [GamepadState::default(); 4],
            gamepad_count: 0,
        }
    }

    /// Check if a specific key is pressed
    pub fn is_key_pressed(&self, key: &str) -> bool {
        self.pressed_keys.iter().any(|k| k == key)
    }

    /// Get gamepad by index
    pub fn get_gamepad(&self, index: u8) -> Option<&GamepadState> {
        if index < self.gamepad_count {
            Some(&self.gamepads[index as usize])
        } else {
            None
        }
    }
}

/// Mouse button state
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseButtonState {
    /// Left button pressed
    pub left: bool,
    /// Right button pressed
    pub right: bool,
    /// Middle button pressed
    pub middle: bool,
}

/// Gamepad state
#[derive(Clone, Copy, Debug, Default)]
pub struct GamepadState {
    /// Whether this gamepad is connected
    pub connected: bool,
    /// Left stick X axis (-1.0 to 1.0)
    pub left_stick_x: f32,
    /// Left stick Y axis (-1.0 to 1.0)
    pub left_stick_y: f32,
    /// Right stick X axis (-1.0 to 1.0)
    pub right_stick_x: f32,
    /// Right stick Y axis (-1.0 to 1.0)
    pub right_stick_y: f32,
    /// Left trigger (0.0 to 1.0)
    pub left_trigger: f32,
    /// Right trigger (0.0 to 1.0)
    pub right_trigger: f32,
    /// Bitmask of pressed buttons
    pub buttons: u32,
}

impl GamepadState {
    /// Standard gamepad button flags
    pub const BUTTON_A: u32 = 1 << 0;
    pub const BUTTON_B: u32 = 1 << 1;
    pub const BUTTON_X: u32 = 1 << 2;
    pub const BUTTON_Y: u32 = 1 << 3;
    pub const BUTTON_LB: u32 = 1 << 4;
    pub const BUTTON_RB: u32 = 1 << 5;
    pub const BUTTON_BACK: u32 = 1 << 6;
    pub const BUTTON_START: u32 = 1 << 7;
    pub const BUTTON_LSTICK: u32 = 1 << 8;
    pub const BUTTON_RSTICK: u32 = 1 << 9;
    pub const DPAD_UP: u32 = 1 << 10;
    pub const DPAD_DOWN: u32 = 1 << 11;
    pub const DPAD_LEFT: u32 = 1 << 12;
    pub const DPAD_RIGHT: u32 = 1 << 13;

    /// Check if a button is pressed
    pub fn is_button_pressed(&self, button: u32) -> bool {
        self.buttons & button != 0
    }

    /// Get left stick as tuple
    pub fn left_stick(&self) -> (f32, f32) {
        (self.left_stick_x, self.left_stick_y)
    }

    /// Get right stick as tuple
    pub fn right_stick(&self) -> (f32, f32) {
        (self.right_stick_x, self.right_stick_y)
    }
}
