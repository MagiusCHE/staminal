//! Window API - Language-agnostic window control
//!
//! Provides APIs for controlling application windows from scripts.
//! All window operations require a WindowHandle obtained from:
//! - `get_main_window()` for the primary window
//! - `create()` for additional windows

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU32, Ordering};

/// ID for the main window (always 0)
pub const MAIN_WINDOW_ID: u32 = 0;

/// Global window ID counter (starts at 1, 0 is reserved for main window)
static NEXT_WINDOW_ID: AtomicU32 = AtomicU32::new(1);

/// Handle to a window
///
/// This handle is used to identify which window to operate on.
/// The main window has id=0, additional windows have id>=1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowHandle {
    /// Unique window ID (0 = main window)
    pub id: u32,
}

impl WindowHandle {
    /// Check if this is the main window
    pub fn is_main(&self) -> bool {
        self.id == MAIN_WINDOW_ID
    }
}

/// Commands to control windows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowCommand {
    /// Create a new window (for windows with id > 0)
    Create {
        id: u32,
        title: String,
        width: u32,
        height: u32,
        resizable: bool,
    },
    /// Set window title
    SetTitle { id: u32, title: String },
    /// Set window size
    SetSize { id: u32, width: u32, height: u32 },
    /// Set fullscreen mode
    SetFullscreen { id: u32, fullscreen: bool },
    /// Set resizable
    SetResizable { id: u32, resizable: bool },
    /// Set window visibility (show/hide)
    SetVisible { id: u32, visible: bool },
    /// Request window close
    RequestClose { id: u32 },
}

/// Window events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowEvent {
    /// Window was resized
    Resized { id: u32, width: u32, height: u32 },
    /// Window gained/lost focus
    Focused { id: u32, focused: bool },
    /// User requested to close the window
    CloseRequested { id: u32 },
}

/// API for controlling application windows
///
/// This struct is passed to scripting runtimes and allows:
/// - Getting the main window handle
/// - Creating additional windows
/// - Controlling window properties (title, size, fullscreen, visibility)
#[derive(Clone)]
pub struct WindowApi {
    command_tx: Sender<WindowCommand>,
    size: Arc<RwLock<(u32, u32)>>,
    title: Arc<RwLock<String>>,
    fullscreen: Arc<RwLock<bool>>,
}

impl WindowApi {
    /// Create a new WindowApi
    pub fn new(
        command_tx: Sender<WindowCommand>,
        size: Arc<RwLock<(u32, u32)>>,
    ) -> Self {
        Self {
            command_tx,
            size,
            title: Arc::new(RwLock::new("Staminal".to_string())),
            fullscreen: Arc::new(RwLock::new(false)),
        }
    }

    /// Get the main window handle
    ///
    /// The main window is created hidden at startup.
    /// Use `show(handle, true)` to make it visible.
    pub fn get_main_window(&self) -> WindowHandle {
        WindowHandle { id: MAIN_WINDOW_ID }
    }

    /// Create a new additional window
    ///
    /// Returns a WindowHandle that can be used to control the window.
    /// The window is created visible by default.
    pub fn create(&self, title: &str, width: u32, height: u32, resizable: bool) -> Result<WindowHandle, String> {
        // Generate unique window ID (1, 2, 3, ...)
        let id = NEXT_WINDOW_ID.fetch_add(1, Ordering::SeqCst);

        // Send create command
        self.command_tx
            .send(WindowCommand::Create {
                id,
                title: title.to_string(),
                width,
                height,
                resizable,
            })
            .map_err(|e| e.to_string())?;

        Ok(WindowHandle { id })
    }

    /// Set window title
    pub fn set_title(&self, handle: WindowHandle, title: &str) -> Result<(), String> {
        // Update local cache for main window
        if handle.is_main() {
            if let Ok(mut t) = self.title.write() {
                *t = title.to_string();
            }
        }
        // Send command
        self.command_tx
            .send(WindowCommand::SetTitle {
                id: handle.id,
                title: title.to_string(),
            })
            .map_err(|e| e.to_string())
    }

    /// Set window size
    pub fn set_size(&self, handle: WindowHandle, width: u32, height: u32) -> Result<(), String> {
        // Update local cache for main window
        if handle.is_main() {
            if let Ok(mut s) = self.size.write() {
                *s = (width, height);
            }
        }
        // Send command
        self.command_tx
            .send(WindowCommand::SetSize {
                id: handle.id,
                width,
                height,
            })
            .map_err(|e| e.to_string())
    }

    /// Set fullscreen mode
    pub fn set_fullscreen(&self, handle: WindowHandle, fullscreen: bool) -> Result<(), String> {
        // Update local cache for main window
        if handle.is_main() {
            if let Ok(mut f) = self.fullscreen.write() {
                *f = fullscreen;
            }
        }
        // Send command
        self.command_tx
            .send(WindowCommand::SetFullscreen {
                id: handle.id,
                fullscreen,
            })
            .map_err(|e| e.to_string())
    }

    /// Set whether the window is resizable
    pub fn set_resizable(&self, handle: WindowHandle, resizable: bool) -> Result<(), String> {
        self.command_tx
            .send(WindowCommand::SetResizable {
                id: handle.id,
                resizable,
            })
            .map_err(|e| e.to_string())
    }

    /// Show or hide the window
    pub fn show(&self, handle: WindowHandle, visible: bool) -> Result<(), String> {
        self.command_tx
            .send(WindowCommand::SetVisible {
                id: handle.id,
                visible,
            })
            .map_err(|e| e.to_string())
    }

    /// Request window close
    pub fn request_close(&self, handle: WindowHandle) -> Result<(), String> {
        self.command_tx
            .send(WindowCommand::RequestClose { id: handle.id })
            .map_err(|e| e.to_string())
    }

    /// Get the current size of the main window
    pub fn get_size(&self) -> (u32, u32) {
        self.size.read().map(|s| *s).unwrap_or((1280, 720))
    }

    /// Get the current title of the main window
    pub fn get_title(&self) -> String {
        self.title.read().map(|t| t.clone()).unwrap_or_else(|_| "Staminal".to_string())
    }

    /// Check if the main window is in fullscreen mode
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen.read().map(|f| *f).unwrap_or(false)
    }

    /// Update the cached size (called by renderer when window is resized)
    pub fn update_size(&self, width: u32, height: u32) {
        if let Ok(mut s) = self.size.write() {
            *s = (width, height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    #[test]
    fn test_window_api() {
        let (tx, rx) = unbounded();
        let size = Arc::new(RwLock::new((800, 600)));
        let api = WindowApi::new(tx, size);

        // Test get_main_window
        let main = api.get_main_window();
        assert_eq!(main.id, MAIN_WINDOW_ID);
        assert!(main.is_main());

        // Test set_title with handle
        api.set_title(main, "Test Window").unwrap();
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, WindowCommand::SetTitle { id: 0, title } if title == "Test Window"));

        // Test get_size
        let (w, h) = api.get_size();
        assert_eq!((w, h), (800, 600));

        // Test set_fullscreen with handle
        api.set_fullscreen(main, true).unwrap();
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, WindowCommand::SetFullscreen { id: 0, fullscreen: true }));

        // Test show
        api.show(main, true).unwrap();
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, WindowCommand::SetVisible { id: 0, visible: true }));

        // Test create new window
        let new_win = api.create("Second Window", 640, 480, true).unwrap();
        assert!(!new_win.is_main());
        assert!(new_win.id >= 1);
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, WindowCommand::Create { id, title, width: 640, height: 480, resizable: true } if id == new_win.id && title == "Second Window"));
    }
}
