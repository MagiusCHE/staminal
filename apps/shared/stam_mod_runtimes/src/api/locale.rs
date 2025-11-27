//! Locale API for Mod Runtimes
//!
//! Provides internationalization support for mods with hierarchical lookup.
//!
//! When a mod requests a localized string via `locale.get(id)` or `locale.get_with_args(id, args)`,
//! the system first checks the mod's own locale files (if present), then falls back to the
//! global application locale.
//!
//! # Mod Locale Structure
//!
//! Mods can include their own translations by creating a `locale/` directory:
//! ```text
//! mods/
//!   my-mod/
//!     mod.json
//!     main.js
//!     locale/
//!       en-US/
//!         main.ftl
//!       it-IT/
//!         main.ftl
//! ```

use fluent::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentValue};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};
use unic_langid::LanguageIdentifier;

/// Thread-safe FluentBundle type using concurrent IntlLangMemoizer
type ConcurrentFluentBundle = FluentBundle<FluentResource>;

/// Strip Unicode bidirectional isolate characters from a string.
/// Fluent inserts these (U+2068 FSI, U+2069 PDI, U+2066 LRI, U+2067 RLI) around placeholders
/// for proper RTL/LTR text handling, but they can cause issues in logs and terminals.
fn strip_bidi_chars(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{2066}' | '\u{2067}' | '\u{2068}' | '\u{2069}'))
        .collect()
}

/// Per-mod locale bundles for a single language
struct ModLocales {
    /// Map of mod_id -> FluentBundle for that mod's locale
    bundles: HashMap<String, ConcurrentFluentBundle>,
}

impl ModLocales {
    fn new() -> Self {
        Self {
            bundles: HashMap::new(),
        }
    }
}

/// Manages per-mod locale bundles with hierarchical fallback to global locale
///
/// This manager stores locale bundles for each mod, allowing mods to provide
/// their own translations that take precedence over global translations.
pub struct ModLocaleManager {
    /// Map of locale_name (e.g., "en-US") -> ModLocales
    locales: RwLock<HashMap<String, ModLocales>>,
    /// Current locale (e.g., "en-US")
    current_locale: RwLock<String>,
    /// Fallback locale
    fallback_locale: String,
}

impl ModLocaleManager {
    /// Create a new ModLocaleManager
    ///
    /// # Arguments
    /// * `current_locale` - The current active locale (e.g., "en-US")
    /// * `fallback_locale` - The fallback locale if current isn't available (e.g., "en-US")
    pub fn new(current_locale: &str, fallback_locale: &str) -> Self {
        Self {
            locales: RwLock::new(HashMap::new()),
            current_locale: RwLock::new(current_locale.to_string()),
            fallback_locale: fallback_locale.to_string(),
        }
    }

    /// Load locale files for a mod from its locale directory
    ///
    /// # Arguments
    /// * `mod_id` - The mod's identifier
    /// * `mod_dir` - Path to the mod's directory (should contain a `locale/` subdirectory)
    pub fn load_mod_locales(&self, mod_id: &str, mod_dir: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let locale_dir = mod_dir.join("locale");

        if !locale_dir.exists() {
            // No locale directory - this is fine, mod just doesn't have translations
            return Ok(());
        }

        debug!("Loading locales for mod '{}' from {:?}", mod_id, locale_dir);

        // Iterate through locale directories (en-US, it-IT, etc.)
        for entry in fs::read_dir(&locale_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(locale_name) = path.file_name().and_then(|n| n.to_str()) {
                    if let Err(e) = self.load_mod_locale(mod_id, locale_name, &path) {
                        warn!("Failed to load locale {} for mod {}: {}", locale_name, mod_id, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a single locale for a mod
    fn load_mod_locale(
        &self,
        mod_id: &str,
        locale_name: &str,
        locale_path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Parse language identifier
        let langid: LanguageIdentifier = locale_name.parse()?;

        // Create bundle with concurrent memoizer for thread-safety
        let mut bundle = FluentBundle::new_concurrent(vec![langid]);

        // Load main.ftl file
        let main_file = locale_path.join("main.ftl");
        if main_file.exists() {
            let ftl_string = fs::read_to_string(&main_file)?;
            let resource = FluentResource::try_new(ftl_string)
                .map_err(|e| format!("Failed to parse FTL for mod {}: {:?}", mod_id, e))?;
            bundle
                .add_resource(resource)
                .map_err(|e| format!("Failed to add resource for mod {}: {:?}", mod_id, e))?;

            debug!("Loaded locale {} for mod {}", locale_name, mod_id);
        } else {
            return Err(format!("main.ftl not found in {:?}", locale_path).into());
        }

        // Store the bundle
        let mut locales = self.locales.write().unwrap();
        let mod_locales = locales
            .entry(locale_name.to_string())
            .or_insert_with(ModLocales::new);
        mod_locales.bundles.insert(mod_id.to_string(), bundle);

        Ok(())
    }

    /// Get a localized message for a specific mod with hierarchical fallback
    ///
    /// Lookup order:
    /// 1. Mod's locale for current language
    /// 2. Mod's locale for fallback language
    /// 3. Returns None (caller should fall back to global locale)
    pub fn get_mod_message(&self, mod_id: &str, id: &str, args: Option<&FluentArgs>) -> Option<String> {
        let current = self.current_locale.read().unwrap().clone();

        // Try current locale first
        if let Some(msg) = self.get_from_mod_locale(mod_id, &current, id, args) {
            return Some(msg);
        }

        // Try fallback locale if different from current
        if current != self.fallback_locale {
            if let Some(msg) = self.get_from_mod_locale(mod_id, &self.fallback_locale, id, args) {
                return Some(msg);
            }
        }

        None
    }

    /// Get a message from a specific mod's locale bundle
    fn get_from_mod_locale(
        &self,
        mod_id: &str,
        locale_name: &str,
        id: &str,
        args: Option<&FluentArgs>,
    ) -> Option<String> {
        let locales = self.locales.read().unwrap();

        let mod_locales = locales.get(locale_name)?;
        let bundle = mod_locales.bundles.get(mod_id)?;
        let message = bundle.get_message(id)?;
        let pattern = message.value()?;

        let mut errors = vec![];
        let value = bundle.format_pattern(pattern, args, &mut errors);

        if !errors.is_empty() {
            warn!("Fluent formatting errors for '{}' in mod '{}': {:?}", id, mod_id, errors);
        }

        Some(strip_bidi_chars(&value))
    }

    /// Update the current locale
    pub fn set_locale(&self, locale: &str) {
        let mut current = self.current_locale.write().unwrap();
        *current = locale.to_string();
    }

    /// Get the current locale
    pub fn current_locale(&self) -> String {
        self.current_locale.read().unwrap().clone()
    }
}

/// Type alias for the global locale get function
type GlobalGetFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Type alias for the global locale get_with_args function
type GlobalGetWithArgsFn = Arc<dyn Fn(&str, &HashMap<String, String>) -> String + Send + Sync>;

/// Locale API wrapper with hierarchical mod-specific locale support
///
/// This API provides two-level locale resolution:
/// 1. First, checks the current mod's locale files (if loaded)
/// 2. Falls back to the global application locale
///
/// The global locale functions are provided via closures to avoid tight coupling
/// with the client's LocaleManager implementation.
#[derive(Clone)]
pub struct LocaleApi {
    /// Manager for mod-specific locales
    mod_locale_manager: Arc<ModLocaleManager>,
    /// Global locale get function (fallback)
    global_get_fn: GlobalGetFn,
    /// Global locale get_with_args function (fallback)
    global_get_with_args_fn: GlobalGetWithArgsFn,
}

impl LocaleApi {
    /// Create a new LocaleApi with mod locale support
    ///
    /// # Arguments
    /// * `current_locale` - The current active locale (e.g., "en-US")
    /// * `fallback_locale` - The fallback locale (e.g., "en-US")
    /// * `global_get_fn` - Function to get messages from global locale
    /// * `global_get_with_args_fn` - Function to get messages with args from global locale
    pub fn new<G, GWA>(
        current_locale: &str,
        fallback_locale: &str,
        global_get_fn: G,
        global_get_with_args_fn: GWA,
    ) -> Self
    where
        G: Fn(&str) -> String + Send + Sync + 'static,
        GWA: Fn(&str, &HashMap<String, String>) -> String + Send + Sync + 'static,
    {
        Self {
            mod_locale_manager: Arc::new(ModLocaleManager::new(current_locale, fallback_locale)),
            global_get_fn: Arc::new(global_get_fn),
            global_get_with_args_fn: Arc::new(global_get_with_args_fn),
        }
    }

    /// Load locale files for a mod
    ///
    /// Should be called when loading a mod, before executing any of its code.
    pub fn load_mod_locales(&self, mod_id: &str, mod_dir: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.mod_locale_manager.load_mod_locales(mod_id, mod_dir)
    }

    /// Get a localized message by ID for a specific mod
    ///
    /// First checks the mod's locale, then falls back to global.
    pub fn get(&self, mod_id: &str, id: &str) -> String {
        // First try mod-specific locale
        if let Some(msg) = self.mod_locale_manager.get_mod_message(mod_id, id, None) {
            return msg;
        }

        // Fall back to global locale
        (self.global_get_fn)(id)
    }

    /// Get a localized message with arguments for a specific mod
    ///
    /// First checks the mod's locale, then falls back to global.
    pub fn get_with_args(&self, mod_id: &str, id: &str, args: &HashMap<String, String>) -> String {
        // Convert HashMap<String, String> to FluentArgs for mod locale lookup
        let mut fluent_args = FluentArgs::new();
        for (key, value) in args {
            fluent_args.set(key.as_str(), FluentValue::from(value.clone()));
        }

        // First try mod-specific locale
        if let Some(msg) = self.mod_locale_manager.get_mod_message(mod_id, id, Some(&fluent_args)) {
            return msg;
        }

        // Fall back to global locale
        (self.global_get_with_args_fn)(id, args)
    }

    /// Update the current locale
    pub fn set_locale(&self, locale: &str) {
        self.mod_locale_manager.set_locale(locale);
    }

    /// Get the current locale
    pub fn current_locale(&self) -> String {
        self.mod_locale_manager.current_locale()
    }

    /// Create a stub LocaleApi that returns message IDs in brackets
    ///
    /// Useful for testing or when no locale system is available.
    pub fn stub() -> Self {
        Self::new(
            "en-US",
            "en-US",
            |id| format!("[{}]", id),
            |id, _args| format!("[{}]", id),
        )
    }
}
