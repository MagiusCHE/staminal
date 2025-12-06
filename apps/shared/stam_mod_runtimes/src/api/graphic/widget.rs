//! Widget System Types
//!
//! Language-agnostic widget types for UI rendering.
//! These types are shared across all scripting runtimes (JS, Lua, C#, etc.)

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ============================================================================
// Widget Type Enum
// ============================================================================

/// Types of widgets supported by the UI system
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetType {
    /// Layout container with flexbox/grid support
    Container,
    /// Static or dynamic text display
    Text,
    /// Clickable button with label
    Button,
    /// Image display from asset
    Image,
    /// Container with background (panel/box)
    Panel,
}

impl std::fmt::Display for WidgetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WidgetType::Container => write!(f, "container"),
            WidgetType::Text => write!(f, "text"),
            WidgetType::Button => write!(f, "button"),
            WidgetType::Image => write!(f, "image"),
            WidgetType::Panel => write!(f, "panel"),
        }
    }
}

impl std::str::FromStr for WidgetType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "container" => Ok(WidgetType::Container),
            "text" => Ok(WidgetType::Text),
            "button" => Ok(WidgetType::Button),
            "image" => Ok(WidgetType::Image),
            "panel" => Ok(WidgetType::Panel),
            _ => Err(format!("Unknown widget type: {}", s)),
        }
    }
}

// ============================================================================
// Size and Layout Types
// ============================================================================

/// Value for widget dimensions (supports px, %, auto)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum SizeValue {
    /// Absolute pixels
    Px(f32),
    /// Percentage of parent
    Percent(f32),
    /// Automatic sizing based on content
    Auto,
}

impl Default for SizeValue {
    fn default() -> Self {
        SizeValue::Auto
    }
}

/// Edge insets for margin, padding, border width
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl EdgeInsets {
    /// Create uniform insets (all sides equal)
    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Create symmetric insets (vertical, horizontal)
    pub fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Create from array [top, right, bottom, left] or [vertical, horizontal] or [all]
    pub fn from_array(values: &[f32]) -> Self {
        match values.len() {
            1 => Self::all(values[0]),
            2 => Self::symmetric(values[0], values[1]),
            4 => Self {
                top: values[0],
                right: values[1],
                bottom: values[2],
                left: values[3],
            },
            _ => Self::default(),
        }
    }
}

/// Layout type for containers
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutType {
    /// Flexbox layout (default)
    #[default]
    Flex,
    /// Grid layout
    Grid,
}

/// Flex direction for flex containers
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlexDirection {
    /// Items laid out in a row (left to right)
    #[default]
    Row,
    /// Items laid out in a column (top to bottom)
    Column,
    /// Items laid out in a row (right to left)
    RowReverse,
    /// Items laid out in a column (bottom to top)
    ColumnReverse,
}

/// Justify content for flex containers (main axis)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JustifyContent {
    /// Default behavior
    Default,
    /// Items packed at start
    #[default]
    FlexStart,
    /// Items packed at end
    FlexEnd,
    /// Items centered
    Center,
    /// Items evenly distributed with space between
    SpaceBetween,
    /// Items evenly distributed with space around
    SpaceAround,
    /// Items evenly distributed with equal space
    SpaceEvenly,
    /// Items stretched to fill
    Stretch,
    /// Items packed at start (logical)
    Start,
    /// Items packed at end (logical)
    End,
}

/// Align items for flex containers (cross axis)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlignItems {
    /// Default behavior
    Default,
    /// Items stretched to fill container
    #[default]
    Stretch,
    /// Items aligned at start
    FlexStart,
    /// Items aligned at end
    FlexEnd,
    /// Items centered
    Center,
    /// Items aligned at baseline
    Baseline,
    /// Items aligned at start (logical)
    Start,
    /// Items aligned at end (logical)
    End,
}

/// Text alignment
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

// ============================================================================
// Color Types
// ============================================================================

/// Color value (RGBA 0.0-1.0) with full transparency support
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorValue {
    /// Red component (0.0-1.0)
    pub r: f32,
    /// Green component (0.0-1.0)
    pub g: f32,
    /// Blue component (0.0-1.0)
    pub b: f32,
    /// Alpha component (0.0 = transparent, 1.0 = opaque)
    pub a: f32,
}

impl Default for ColorValue {
    fn default() -> Self {
        Self::white()
    }
}

impl ColorValue {
    /// Create color from RGBA values (0.0-1.0)
    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Create color from RGB values (0.0-1.0), fully opaque
    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Create color from hex string
    /// Supports: "#RGB", "#RGBA", "#RRGGBB", "#RRGGBBAA", "rgba(r,g,b,a)"
    pub fn from_hex(hex: &str) -> Result<Self, ColorParseError> {
        let hex = hex.trim();

        // Handle rgba() format
        if hex.starts_with("rgba(") && hex.ends_with(')') {
            return Self::parse_rgba_function(hex);
        }

        // Handle rgb() format
        if hex.starts_with("rgb(") && hex.ends_with(')') {
            return Self::parse_rgb_function(hex);
        }

        // Handle hex format
        let hex = hex.trim_start_matches('#');

        match hex.len() {
            // #RGB
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16)
                    .map_err(|_| ColorParseError::InvalidHex)?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16)
                    .map_err(|_| ColorParseError::InvalidHex)?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16)
                    .map_err(|_| ColorParseError::InvalidHex)?;
                Ok(Self::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
            }
            // #RGBA
            4 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16)
                    .map_err(|_| ColorParseError::InvalidHex)?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16)
                    .map_err(|_| ColorParseError::InvalidHex)?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16)
                    .map_err(|_| ColorParseError::InvalidHex)?;
                let a = u8::from_str_radix(&hex[3..4].repeat(2), 16)
                    .map_err(|_| ColorParseError::InvalidHex)?;
                Ok(Self::rgba(
                    r as f32 / 255.0,
                    g as f32 / 255.0,
                    b as f32 / 255.0,
                    a as f32 / 255.0,
                ))
            }
            // #RRGGBB
            6 => {
                let r =
                    u8::from_str_radix(&hex[0..2], 16).map_err(|_| ColorParseError::InvalidHex)?;
                let g =
                    u8::from_str_radix(&hex[2..4], 16).map_err(|_| ColorParseError::InvalidHex)?;
                let b =
                    u8::from_str_radix(&hex[4..6], 16).map_err(|_| ColorParseError::InvalidHex)?;
                Ok(Self::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
            }
            // #RRGGBBAA
            8 => {
                let r =
                    u8::from_str_radix(&hex[0..2], 16).map_err(|_| ColorParseError::InvalidHex)?;
                let g =
                    u8::from_str_radix(&hex[2..4], 16).map_err(|_| ColorParseError::InvalidHex)?;
                let b =
                    u8::from_str_radix(&hex[4..6], 16).map_err(|_| ColorParseError::InvalidHex)?;
                let a =
                    u8::from_str_radix(&hex[6..8], 16).map_err(|_| ColorParseError::InvalidHex)?;
                Ok(Self::rgba(
                    r as f32 / 255.0,
                    g as f32 / 255.0,
                    b as f32 / 255.0,
                    a as f32 / 255.0,
                ))
            }
            _ => Err(ColorParseError::InvalidLength),
        }
    }

    fn parse_rgba_function(s: &str) -> Result<Self, ColorParseError> {
        let inner = s
            .trim_start_matches("rgba(")
            .trim_end_matches(')')
            .trim();
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();

        if parts.len() != 4 {
            return Err(ColorParseError::InvalidRgbaFormat);
        }

        let r: f32 = parts[0].parse().map_err(|_| ColorParseError::InvalidRgbaFormat)?;
        let g: f32 = parts[1].parse().map_err(|_| ColorParseError::InvalidRgbaFormat)?;
        let b: f32 = parts[2].parse().map_err(|_| ColorParseError::InvalidRgbaFormat)?;
        let a: f32 = parts[3].parse().map_err(|_| ColorParseError::InvalidRgbaFormat)?;

        // If values are > 1, assume they're in 0-255 range
        let (r, g, b) = if r > 1.0 || g > 1.0 || b > 1.0 {
            (r / 255.0, g / 255.0, b / 255.0)
        } else {
            (r, g, b)
        };

        Ok(Self::rgba(r, g, b, a))
    }

    fn parse_rgb_function(s: &str) -> Result<Self, ColorParseError> {
        let inner = s.trim_start_matches("rgb(").trim_end_matches(')').trim();
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();

        if parts.len() != 3 {
            return Err(ColorParseError::InvalidRgbFormat);
        }

        let r: f32 = parts[0].parse().map_err(|_| ColorParseError::InvalidRgbFormat)?;
        let g: f32 = parts[1].parse().map_err(|_| ColorParseError::InvalidRgbFormat)?;
        let b: f32 = parts[2].parse().map_err(|_| ColorParseError::InvalidRgbFormat)?;

        // If values are > 1, assume they're in 0-255 range
        let (r, g, b) = if r > 1.0 || g > 1.0 || b > 1.0 {
            (r / 255.0, g / 255.0, b / 255.0)
        } else {
            (r, g, b)
        };

        Ok(Self::rgb(r, g, b))
    }

    /// Create color with specified alpha
    pub fn with_alpha(mut self, alpha: f32) -> Self {
        self.a = alpha;
        self
    }

    /// Completely transparent color
    pub fn transparent() -> Self {
        Self {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }
    }

    /// White color
    pub fn white() -> Self {
        Self {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        }
    }

    /// Black color
    pub fn black() -> Self {
        Self {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }
}

/// Error when parsing color strings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorParseError {
    InvalidHex,
    InvalidLength,
    InvalidRgbaFormat,
    InvalidRgbFormat,
}

impl std::fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorParseError::InvalidHex => write!(f, "Invalid hex color format"),
            ColorParseError::InvalidLength => write!(f, "Invalid color string length"),
            ColorParseError::InvalidRgbaFormat => write!(f, "Invalid rgba() format"),
            ColorParseError::InvalidRgbFormat => write!(f, "Invalid rgb() format"),
        }
    }
}

impl std::error::Error for ColorParseError {}

// ============================================================================
// Blend Mode
// ============================================================================

/// Blend mode for advanced graphics effects
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlendMode {
    /// Normal alpha blending (default)
    #[default]
    Normal,
    /// Multiply colors (darkens)
    Multiply,
    /// Screen blend (lightens)
    Screen,
    /// Overlay (combination of multiply and screen)
    Overlay,
    /// Additive blend (adds brightness)
    Add,
    /// Subtractive blend
    Subtract,
}

// ============================================================================
// Image Types
// ============================================================================

/// Image scale mode for Image widgets
///
/// Maps to Bevy's `NodeImageMode` variants plus additional CSS-like modes.
/// The variants correspond to how the image is scaled within its container.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImageScaleMode {
    /// Automatic sizing based on image's natural dimensions (Bevy default)
    /// The image keeps its natural size and aspect ratio.
    Auto,
    /// Stretch to fill the container (ignores aspect ratio)
    /// Maps to Bevy's `NodeImageMode::Stretch`.
    Stretch,
    /// Repeat the image as a pattern (tile)
    /// Maps to Bevy's `NodeImageMode::Tiled`.
    Tiled {
        /// Whether to tile horizontally
        #[serde(default = "default_true")]
        tile_x: bool,
        /// Whether to tile vertically
        #[serde(default = "default_true")]
        tile_y: bool,
        /// Stretch factor for tiles (1.0 = no stretch)
        #[serde(default = "default_one")]
        stretch_value: f32,
    },
    /// 9-slice scaling for UI elements (preserves corners)
    /// Maps to Bevy's `NodeImageMode::Sliced`.
    Sliced {
        /// Border size from the top edge (in pixels)
        top: f32,
        /// Border size from the right edge (in pixels)
        right: f32,
        /// Border size from the bottom edge (in pixels)
        bottom: f32,
        /// Border size from the left edge (in pixels)
        left: f32,
        /// Whether the center portion should be drawn
        #[serde(default = "default_true")]
        center: bool,
    },
    /// Scale to fit within bounds while maintaining aspect ratio (may show background/letterbox)
    /// The entire image is visible, but the container may have empty space.
    /// This is NOT a native Bevy mode - implemented via custom sizing.
    Contain,
    /// Scale to cover the entire container while maintaining aspect ratio (may crop)
    /// The container is fully covered, but parts of the image may be clipped.
    /// This is NOT a native Bevy mode - implemented via custom sizing.
    Cover,
}

fn default_true() -> bool {
    true
}

fn default_one() -> f32 {
    1.0
}

impl Default for ImageScaleMode {
    fn default() -> Self {
        ImageScaleMode::Auto
    }
}

impl ImageScaleMode {
    /// Convert ImageScaleMode variant to a u32 identifier
    ///
    /// Used for JS bindings. The mapping is:
    /// - Auto = 0
    /// - Stretch = 1
    /// - Tiled = 2
    /// - Sliced = 3
    /// - Contain = 4
    /// - Cover = 5
    pub fn variant_to_u32(&self) -> u32 {
        match self {
            ImageScaleMode::Auto => 0,
            ImageScaleMode::Stretch => 1,
            ImageScaleMode::Tiled { .. } => 2,
            ImageScaleMode::Sliced { .. } => 3,
            ImageScaleMode::Contain => 4,
            ImageScaleMode::Cover => 5,
        }
    }
}

/// Rectangle for sprite sheet source regions
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RectValue {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Image configuration (for background or Image widget)
///
/// Images can be specified in two ways:
/// 1. **Direct path**: Set `path` to load the image directly (loaded on demand)
/// 2. **Resource ID**: Set `resource_id` to use a pre-loaded resource (via `Resource.load()`)
///
/// If both `path` and `resource_id` are provided, `resource_id` takes precedence.
/// Using `resource_id` is recommended for better performance as resources are pre-cached.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageConfig {
    /// Asset path (relative to mod or asset folder)
    /// Optional if `resource_id` is provided
    #[serde(default)]
    pub path: Option<String>,
    /// Resource ID (alias from Resource.load())
    /// Takes precedence over `path` if both are provided
    #[serde(default)]
    pub resource_id: Option<String>,
    /// Scale mode
    #[serde(default)]
    pub scale_mode: ImageScaleMode,
    /// Tint color (multiplied with image pixels)
    pub tint: Option<ColorValue>,
    /// Image opacity (0.0-1.0)
    pub opacity: Option<f32>,
    /// Flip horizontally
    #[serde(default)]
    pub flip_x: bool,
    /// Flip vertically
    #[serde(default)]
    pub flip_y: bool,
    /// Source rectangle for sprite sheets
    pub source_rect: Option<RectValue>,
}

impl ImageConfig {
    /// Create ImageConfig from a direct path
    pub fn from_path(path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            resource_id: None,
            scale_mode: ImageScaleMode::default(),
            tint: None,
            opacity: None,
            flip_x: false,
            flip_y: false,
            source_rect: None,
        }
    }

    /// Create ImageConfig from a resource ID
    pub fn from_resource(resource_id: impl Into<String>) -> Self {
        Self {
            path: None,
            resource_id: Some(resource_id.into()),
            scale_mode: ImageScaleMode::default(),
            tint: None,
            opacity: None,
            flip_x: false,
            flip_y: false,
            source_rect: None,
        }
    }

    /// Check if this config has a valid image source (either path or resource_id)
    pub fn has_source(&self) -> bool {
        self.path.is_some() || self.resource_id.is_some()
    }

    /// Get the effective source: resource_id takes precedence over path
    pub fn effective_source(&self) -> Option<ImageSource> {
        if let Some(ref resource_id) = self.resource_id {
            Some(ImageSource::ResourceId(resource_id.clone()))
        } else if let Some(ref path) = self.path {
            Some(ImageSource::Path(path.clone()))
        } else {
            None
        }
    }
}

/// Represents the source of an image
#[derive(Clone, Debug)]
pub enum ImageSource {
    /// Direct file path
    Path(String),
    /// Resource ID (pre-loaded via Resource.load())
    ResourceId(String),
}

// ============================================================================
// Font Types
// ============================================================================

/// Font weight
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    Thin,     // 100
    Light,    // 300
    #[default]
    Regular, // 400
    Medium,   // 500
    SemiBold, // 600
    Bold,     // 700
    ExtraBold, // 800
    Black,    // 900
}

/// Font style
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// Font configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontConfig {
    /// Font family name or path (e.g., "Roboto", "fonts/custom.ttf")
    pub family: String,
    /// Size in pixels
    pub size: f32,
    /// Font weight
    #[serde(default)]
    pub weight: FontWeight,
    /// Font style
    #[serde(default)]
    pub style: FontStyle,
    /// Letter spacing
    pub letter_spacing: Option<f32>,
    /// Line height multiplier
    pub line_height: Option<f32>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "default".to_string(),
            size: 16.0,
            weight: FontWeight::Regular,
            style: FontStyle::Normal,
            letter_spacing: None,
            line_height: None,
        }
    }
}

/// Information about a loaded font
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontInfo {
    /// Alias used to reference this font
    pub alias: String,
    /// Original path
    pub path: String,
    /// Internal family name if available
    pub family_name: Option<String>,
}

// ============================================================================
// Shadow Types
// ============================================================================

/// Shadow configuration (for text or widget shadows)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShadowConfig {
    /// Shadow color (RGBA with alpha)
    pub color: ColorValue,
    /// Horizontal offset
    pub offset_x: f32,
    /// Vertical offset
    pub offset_y: f32,
    /// Blur radius (optional)
    pub blur_radius: Option<f32>,
}

// ============================================================================
// Widget Configuration
// ============================================================================

/// Complete widget configuration (serializable for all runtimes)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WidgetConfig {
    /// Parent widget ID (None = root of window)
    pub parent_id: Option<u64>,

    // === Layout ===
    /// Layout type (flex/grid)
    pub layout: Option<LayoutType>,
    /// Flex direction
    pub direction: Option<FlexDirection>,
    /// Justify content (main axis)
    pub justify_content: Option<JustifyContent>,
    /// Align items (cross axis)
    pub align_items: Option<AlignItems>,
    /// Gap between items
    pub gap: Option<f32>,

    // === Dimensions ===
    /// Widget width
    pub width: Option<SizeValue>,
    /// Widget height
    pub height: Option<SizeValue>,
    /// Minimum width
    pub min_width: Option<SizeValue>,
    /// Maximum width
    pub max_width: Option<SizeValue>,
    /// Minimum height
    pub min_height: Option<SizeValue>,
    /// Maximum height
    pub max_height: Option<SizeValue>,

    // === Spacing ===
    /// Outer margin
    pub margin: Option<EdgeInsets>,
    /// Inner padding
    pub padding: Option<EdgeInsets>,

    // === Appearance ===
    /// Background color (RGBA with alpha)
    pub background_color: Option<ColorValue>,
    /// Border color (RGBA with alpha)
    pub border_color: Option<ColorValue>,
    /// Border width
    pub border_width: Option<EdgeInsets>,
    /// Border radius
    pub border_radius: Option<f32>,
    /// Widget opacity (0.0-1.0)
    pub opacity: Option<f32>,
    /// Blend mode
    pub blend_mode: Option<BlendMode>,

    // === Background Image ===
    /// Background image (alternative to background_color)
    pub background_image: Option<ImageConfig>,

    // === Text Properties ===
    /// Text content
    pub content: Option<String>,
    /// Font configuration
    pub font: Option<FontConfig>,
    /// Text color (RGBA with alpha)
    pub font_color: Option<ColorValue>,
    /// Text alignment
    pub text_align: Option<TextAlign>,
    /// Text shadow
    pub text_shadow: Option<ShadowConfig>,

    // === Button Properties ===
    /// Button label
    pub label: Option<String>,
    /// Hover color (RGBA with alpha)
    pub hover_color: Option<ColorValue>,
    /// Pressed color (RGBA with alpha)
    pub pressed_color: Option<ColorValue>,
    /// Disabled state
    pub disabled: Option<bool>,
    /// Disabled color
    pub disabled_color: Option<ColorValue>,

    // === Image Widget Properties ===
    /// Image configuration (for Image widget type)
    pub image: Option<ImageConfig>,
}

// ============================================================================
// Widget Info (Query Result)
// ============================================================================

/// Information about a widget (returned by queries)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WidgetInfo {
    /// Unique widget ID
    pub id: u64,
    /// Parent window ID
    pub window_id: u64,
    /// Widget type
    pub widget_type: WidgetType,
    /// Parent widget ID (None if root)
    pub parent_id: Option<u64>,
    /// IDs of child widgets
    pub children_ids: Vec<u64>,
}

// ============================================================================
// Property Value (Dynamic Updates)
// ============================================================================

/// Dynamic property value for widget updates
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
    Color(ColorValue),
    Size(SizeValue),
}

// ============================================================================
// Widget Events
// ============================================================================

/// Types of widget events that can be subscribed to
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetEventType {
    /// Mouse click on widget
    Click,
    /// Mouse hover enter/leave
    Hover,
    /// Focus gained/lost
    Focus,
}

/// Widget event subscription registry
#[derive(Clone, Debug, Default)]
pub struct WidgetSubscriptions {
    /// Map of widget ID to subscribed event types
    subscriptions: std::collections::HashMap<u64, HashSet<WidgetEventType>>,
}

impl WidgetSubscriptions {
    /// Create new empty subscriptions
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to an event type for a widget
    pub fn subscribe(&mut self, widget_id: u64, event_type: WidgetEventType) {
        self.subscriptions
            .entry(widget_id)
            .or_default()
            .insert(event_type);
    }

    /// Unsubscribe from an event type for a widget
    pub fn unsubscribe(&mut self, widget_id: u64, event_type: WidgetEventType) {
        if let Some(events) = self.subscriptions.get_mut(&widget_id) {
            events.remove(&event_type);
            if events.is_empty() {
                self.subscriptions.remove(&widget_id);
            }
        }
    }

    /// Check if a widget is subscribed to an event type
    pub fn is_subscribed(&self, widget_id: u64, event_type: WidgetEventType) -> bool {
        self.subscriptions
            .get(&widget_id)
            .map(|events| events.contains(&event_type))
            .unwrap_or(false)
    }

    /// Remove all subscriptions for a widget
    pub fn remove_widget(&mut self, widget_id: u64) {
        self.subscriptions.remove(&widget_id);
    }

    /// Get all subscribed event types for a widget
    pub fn get_subscriptions(&self, widget_id: u64) -> Option<&HashSet<WidgetEventType>> {
        self.subscriptions.get(&widget_id)
    }
}

// ============================================================================
// Widget Filter (for queries)
// ============================================================================

/// Filter for widget queries
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WidgetFilter {
    /// Filter by widget type
    pub widget_type: Option<WidgetType>,
    /// Filter by parent ID
    pub parent_id: Option<Option<u64>>,
}
