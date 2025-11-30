//! JavaScript bindings for the Graphic API
//!
//! This module provides JavaScript bindings for the graphic system,
//! allowing mods to create windows, handle input, and interact with
//! the graphic engine (Bevy, WGPU, etc.).

use rquickjs::{Array, Ctx, Function, JsLifetime, Object, class::Trace, function::Opt};
use crate::api::graphic::{GraphicApi, GraphicEngines, WindowConfig, WindowPositionMode};

/// JavaScript Graphic API class
///
/// This class is exposed to JavaScript as the `graphic` global object.
/// It provides methods to enable graphic engines, create windows, and handle input.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct GraphicJS {
    #[qjs(skip_trace)]
    graphic_api: GraphicApi,
}

#[rquickjs::methods]
impl GraphicJS {
    /// Enable a graphic engine by type
    ///
    /// This method enables the specified graphic engine by looking up
    /// the registered factory for that engine type and starting the engine
    /// in a separate thread.
    ///
    /// # Arguments
    /// * `engine_type` - The engine type to enable (GraphicEngines enum value)
    ///
    /// # Returns
    /// Promise that resolves when the engine is started, or rejects on error
    ///
    /// # Note
    /// This method only works if the engine factory has been pre-registered from Rust.
    /// On the client, BevyEngine is registered automatically at startup.
    #[qjs(rename = "enableEngine")]
    pub async fn enable_engine<'js>(&self, ctx: Ctx<'js>, engine_type: u32) -> rquickjs::Result<()> {
        let engine = GraphicEngines::from_u32(engine_type).ok_or_else(|| {
            let msg = format!("Invalid engine type: {}. Valid types are: Bevy(0), Wgpu(1), Terminal(2)", engine_type);
            tracing::error!("{}", msg);
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &msg).unwrap().into())
        })?;

        tracing::debug!("GraphicJS::enable_engine called with engine type: {:?}", engine);

        self.graphic_api.enable_by_type(engine).await.map_err(|e| {
            tracing::error!("Failed to enable graphic engine: {}", e);
            ctx.throw(rquickjs::String::from_str(ctx.clone(), &e).unwrap().into())
        })
    }

    /// Check if a graphic engine is currently enabled
    #[qjs(rename = "isEnabled")]
    pub fn is_enabled(&self) -> bool {
        self.graphic_api.is_enabled()
    }

    /// Check if the graphic engine is ready to receive commands
    #[qjs(rename = "isReady")]
    pub fn is_ready(&self) -> bool {
        self.graphic_api.is_ready()
    }

    /// Get the currently active engine type
    ///
    /// # Returns
    /// The engine type number (GraphicEngines enum value), or null if no engine is enabled
    #[qjs(rename = "getEngine")]
    pub fn get_engine(&self) -> Option<u32> {
        self.graphic_api.active_engine().map(|e| e as u32)
    }

    /// Create a new window
    ///
    /// # Arguments
    /// * `config` - Optional window configuration object with:
    ///   - title: string (default: "Staminal")
    ///   - width: number (default: 1280)
    ///   - height: number (default: 720)
    ///   - fullscreen: boolean (default: false)
    ///   - resizable: boolean (default: true)
    ///   - visible: boolean (default: true)
    ///
    /// # Returns
    /// Promise that resolves to a WindowJS instance
    #[qjs(rename = "createWindow")]
    pub async fn create_window<'js>(
        &self,
        ctx: Ctx<'js>,
        config: Opt<Object<'js>>,
    ) -> rquickjs::Result<rquickjs::Class<'js, WindowJS>> {
        let mut window_config = WindowConfig::default();

        if let Some(obj) = config.0 {
            if let Ok(title) = obj.get::<_, String>("title") {
                window_config.title = title;
            }
            if let Ok(width) = obj.get::<_, u32>("width") {
                window_config.width = width;
            }
            if let Ok(height) = obj.get::<_, u32>("height") {
                window_config.height = height;
            }
            if let Ok(fullscreen) = obj.get::<_, bool>("fullscreen") {
                window_config.fullscreen = fullscreen;
            }
            if let Ok(resizable) = obj.get::<_, bool>("resizable") {
                window_config.resizable = resizable;
            }
            if let Ok(visible) = obj.get::<_, bool>("visible") {
                window_config.visible = visible;
            }
        }

        let window_id = self
            .graphic_api
            .create_window(window_config.clone())
            .await
            .map_err(|e| {
                tracing::error!("Failed to create window: {}", e);
                rquickjs::Error::Exception
            })?;

        // Create WindowJS instance
        let window_js = WindowJS {
            graphic_api: self.graphic_api.clone(),
            window_id,
            title: window_config.title,
            width: window_config.width,
            height: window_config.height,
        };

        rquickjs::Class::<WindowJS>::instance(ctx, window_js)
    }

    /// Get current mouse position
    ///
    /// When called inside a frame callback, returns cached snapshot data (O(1)).
    /// When called outside, makes a synchronous request to the engine.
    ///
    /// # Returns
    /// Object with x and y properties
    #[qjs(rename = "getMousePosition")]
    pub fn get_mouse_position<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Object<'js>> {
        let snapshot = self.graphic_api.frame_snapshot();
        let obj = Object::new(ctx)?;
        obj.set("x", snapshot.mouse_x)?;
        obj.set("y", snapshot.mouse_y)?;
        Ok(obj)
    }

    /// Get current mouse button states
    ///
    /// # Returns
    /// Object with left, right, middle boolean properties
    #[qjs(rename = "getMouseButtons")]
    pub fn get_mouse_buttons<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Object<'js>> {
        let snapshot = self.graphic_api.frame_snapshot();
        let obj = Object::new(ctx)?;
        obj.set("left", snapshot.mouse_buttons.left)?;
        obj.set("right", snapshot.mouse_buttons.right)?;
        obj.set("middle", snapshot.mouse_buttons.middle)?;
        Ok(obj)
    }

    /// Check if a specific key is currently pressed
    ///
    /// # Arguments
    /// * `key` - The key code string (e.g., "KeyW", "Space", "Escape")
    ///
    /// # Returns
    /// true if the key is pressed
    #[qjs(rename = "isKeyPressed")]
    pub fn is_key_pressed(&self, key: String) -> bool {
        let snapshot = self.graphic_api.frame_snapshot();
        snapshot.is_key_pressed(&key)
    }

    /// Get all currently pressed keys
    ///
    /// # Returns
    /// Array of key code strings
    #[qjs(rename = "getPressedKeys")]
    pub fn get_pressed_keys<'js>(&self, ctx: Ctx<'js>) -> rquickjs::Result<Array<'js>> {
        let snapshot = self.graphic_api.frame_snapshot();
        let array = Array::new(ctx)?;
        for (idx, key) in snapshot.pressed_keys.iter().enumerate() {
            array.set(idx, key.as_str())?;
        }
        Ok(array)
    }

    /// Get gamepad state by index
    ///
    /// # Arguments
    /// * `index` - Gamepad index (0-3)
    ///
    /// # Returns
    /// Gamepad state object or null if not connected
    #[qjs(rename = "getGamepad")]
    pub fn get_gamepad<'js>(&self, ctx: Ctx<'js>, index: u8) -> rquickjs::Result<rquickjs::Value<'js>> {
        let snapshot = self.graphic_api.frame_snapshot();

        if let Some(gamepad) = snapshot.get_gamepad(index) {
            if gamepad.connected {
                let obj = Object::new(ctx.clone())?;
                obj.set("connected", true)?;

                // Left stick
                let left_stick = Array::new(ctx.clone())?;
                left_stick.set(0, gamepad.left_stick_x)?;
                left_stick.set(1, gamepad.left_stick_y)?;
                obj.set("leftStick", left_stick)?;

                // Right stick
                let right_stick = Array::new(ctx.clone())?;
                right_stick.set(0, gamepad.right_stick_x)?;
                right_stick.set(1, gamepad.right_stick_y)?;
                obj.set("rightStick", right_stick)?;

                // Triggers
                obj.set("leftTrigger", gamepad.left_trigger)?;
                obj.set("rightTrigger", gamepad.right_trigger)?;

                // Button helper function
                let buttons = gamepad.buttons;
                let is_button_pressed_fn = Function::new(ctx.clone(), move |_ctx: Ctx, button: String| -> bool {
                    use crate::api::graphic::GamepadState;
                    match button.as_str() {
                        "A" => buttons & GamepadState::BUTTON_A != 0,
                        "B" => buttons & GamepadState::BUTTON_B != 0,
                        "X" => buttons & GamepadState::BUTTON_X != 0,
                        "Y" => buttons & GamepadState::BUTTON_Y != 0,
                        "LB" => buttons & GamepadState::BUTTON_LB != 0,
                        "RB" => buttons & GamepadState::BUTTON_RB != 0,
                        "Back" => buttons & GamepadState::BUTTON_BACK != 0,
                        "Start" => buttons & GamepadState::BUTTON_START != 0,
                        "LStick" => buttons & GamepadState::BUTTON_LSTICK != 0,
                        "RStick" => buttons & GamepadState::BUTTON_RSTICK != 0,
                        "DPadUp" => buttons & GamepadState::DPAD_UP != 0,
                        "DPadDown" => buttons & GamepadState::DPAD_DOWN != 0,
                        "DPadLeft" => buttons & GamepadState::DPAD_LEFT != 0,
                        "DPadRight" => buttons & GamepadState::DPAD_RIGHT != 0,
                        _ => false,
                    }
                })?;
                obj.set("isButtonPressed", is_button_pressed_fn)?;

                return Ok(obj.into_value());
            }
        }

        Ok(rquickjs::Null.into_value(ctx))
    }

    /// Get current frame number
    #[qjs(rename = "getFrameNumber")]
    pub fn get_frame_number(&self) -> u64 {
        self.graphic_api.frame_snapshot().frame_number
    }

    /// Shutdown the graphic engine gracefully
    ///
    /// # Returns
    /// Promise that resolves when shutdown is complete
    #[qjs(rename = "shutdown")]
    pub async fn shutdown(&self) -> rquickjs::Result<()> {
        self.graphic_api
            .shutdown(std::time::Duration::from_secs(5))
            .await
            .map_err(|e| {
                tracing::error!("Failed to shutdown graphic engine: {}", e);
                rquickjs::Error::Exception
            })
    }
}

/// JavaScript Window class
///
/// Represents a window created by the graphic system.
/// Provides methods to manipulate the window properties.
#[rquickjs::class]
#[derive(Clone, Trace, JsLifetime)]
pub struct WindowJS {
    #[qjs(skip_trace)]
    graphic_api: GraphicApi,
    #[qjs(skip_trace)]
    window_id: u64,
    #[qjs(skip_trace)]
    title: String,
    #[qjs(skip_trace)]
    width: u32,
    #[qjs(skip_trace)]
    height: u32,
}

#[rquickjs::methods]
impl WindowJS {
    /// Get the window ID
    #[qjs(get, rename = "id")]
    pub fn id(&self) -> u64 {
        self.window_id
    }

    /// Get the window title
    #[qjs(get, rename = "title")]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// Get the window width
    #[qjs(get, rename = "width")]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get the window height
    #[qjs(get, rename = "height")]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Set the window size
    #[qjs(rename = "setSize")]
    pub async fn set_size(&mut self, width: u32, height: u32) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_size(self.window_id, width, height)
            .await
            .map_err(|e| {
                tracing::error!("Failed to set window size: {}", e);
                rquickjs::Error::Exception
            })?;

        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Set the window title
    #[qjs(rename = "setTitle")]
    pub async fn set_title(&mut self, title: String) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_title(self.window_id, title.clone())
            .await
            .map_err(|e| {
                tracing::error!("Failed to set window title: {}", e);
                rquickjs::Error::Exception
            })?;

        self.title = title;
        Ok(())
    }

    /// Set fullscreen mode
    #[qjs(rename = "setFullscreen")]
    pub async fn set_fullscreen(&self, fullscreen: bool) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_fullscreen(self.window_id, fullscreen)
            .await
            .map_err(|e| {
                tracing::error!("Failed to set fullscreen: {}", e);
                rquickjs::Error::Exception
            })
    }

    /// Set window visibility
    #[qjs(rename = "setVisible")]
    pub async fn set_visible(&self, visible: bool) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_visible(self.window_id, visible)
            .await
            .map_err(|e| {
                tracing::error!("Failed to set visibility: {}", e);
                rquickjs::Error::Exception
            })
    }

    /// Set window position
    #[qjs(rename = "setPosition")]
    pub async fn set_position(&self, x: i32, y: i32) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_position(self.window_id, x, y)
            .await
            .map_err(|e| {
                tracing::error!("Failed to set position: {}", e);
                rquickjs::Error::Exception
            })
    }

    /// Set window position mode (centered, default, manual)
    #[qjs(rename = "setPositionMode")]
    pub async fn set_position_mode(&self, mode: u32) -> rquickjs::Result<()> {
        let position_mode = WindowPositionMode::from_u32(mode).ok_or_else(|| {
            tracing::error!("Invalid position mode: {}", mode);
            rquickjs::Error::Exception
        })?;

        self.graphic_api
            .set_window_position_mode(self.window_id, position_mode)
            .await
            .map_err(|e| {
                tracing::error!("Failed to set position mode: {}", e);
                rquickjs::Error::Exception
            })
    }

    /// Set whether the window is resizable
    #[qjs(rename = "setResizable")]
    pub async fn set_resizable(&self, resizable: bool) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_resizable(self.window_id, resizable)
            .await
            .map_err(|e| {
                tracing::error!("Failed to set resizable: {}", e);
                rquickjs::Error::Exception
            })
    }

    /// Show the window (make it visible)
    #[qjs(rename = "show")]
    pub async fn show(&self) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_visible(self.window_id, true)
            .await
            .map_err(|e| {
                tracing::error!("Failed to show window: {}", e);
                rquickjs::Error::Exception
            })
    }

    /// Hide the window
    #[qjs(rename = "hide")]
    pub async fn hide(&self) -> rquickjs::Result<()> {
        self.graphic_api
            .set_window_visible(self.window_id, false)
            .await
            .map_err(|e| {
                tracing::error!("Failed to hide window: {}", e);
                rquickjs::Error::Exception
            })
    }

    /// Close the window
    #[qjs(rename = "close")]
    pub async fn close(&self) -> rquickjs::Result<()> {
        self.graphic_api
            .close_window(self.window_id)
            .await
            .map_err(|e| {
                tracing::error!("Failed to close window: {}", e);
                rquickjs::Error::Exception
            })
    }
}

/// Setup graphic API in the JavaScript context
///
/// Provides:
/// - `graphic` global object with methods for engine control and input
/// - `GraphicEngines` enum object with Bevy, Wgpu, Terminal values
/// - `WindowJS` class for window instances
/// - `system.enable_graphic_engine()` alias for backward compatibility
pub fn setup_graphic_api(ctx: Ctx, graphic_api: GraphicApi) -> Result<(), rquickjs::Error> {
    // Define WindowJS class
    rquickjs::Class::<WindowJS>::define(&ctx.globals())?;

    // Define GraphicJS class
    rquickjs::Class::<GraphicJS>::define(&ctx.globals())?;

    // Create an instance of GraphicJS
    let graphic_obj = rquickjs::Class::<GraphicJS>::instance(ctx.clone(), GraphicJS { graphic_api })?;

    // Register it as global 'graphic' object
    ctx.globals().set("graphic", graphic_obj)?;

    // Create GraphicEngines enum object
    let engines = Object::new(ctx.clone())?;
    engines.set("Bevy", GraphicEngines::Bevy as u32)?;
    engines.set("Wgpu", GraphicEngines::Wgpu as u32)?;
    engines.set("Terminal", GraphicEngines::Terminal as u32)?;
    ctx.globals().set("GraphicEngines", engines)?;

    // Create WindowPositionModes enum object
    let position_modes = Object::new(ctx.clone())?;
    position_modes.set("Default", WindowPositionMode::Default as u32)?;
    position_modes.set("Centered", WindowPositionMode::Centered as u32)?;
    position_modes.set("Manual", WindowPositionMode::Manual as u32)?;
    ctx.globals().set("WindowPositionModes", position_modes)?;

    // Add system.enableGraphicEngine() alias for convenience
    // This allows scripts to use: await system.enableGraphicEngine(GraphicEngines.Bevy)
    // instead of: await graphic.enableEngine(GraphicEngines.Bevy)
    // We use eval to create a JS wrapper function that calls the async method
    ctx.eval::<(), _>(r#"
        system.enableGraphicEngine = function(engine_type) {
            return graphic.enableEngine(engine_type);
        };
    "#)?;

    Ok(())
}
