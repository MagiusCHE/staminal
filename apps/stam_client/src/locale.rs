use fluent::{FluentBundle, FluentResource};
use fluent_bundle::{FluentArgs, FluentValue};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use unic_langid::LanguageIdentifier;
use tracing::{info, warn, error};

/// Locale manager for internationalization
pub struct LocaleManager {
    bundles: HashMap<String, FluentBundle<FluentResource>>,
    current_locale: String,
    fallback_locale: String,
}

impl LocaleManager {
    /// Create a new LocaleManager and load locales from assets directory
    ///
    /// # Arguments
    /// * `assets_path` - Path to assets directory containing locales
    /// * `preferred_locale` - Optional locale to use (overrides system locale)
    pub fn new(assets_path: &str, preferred_locale: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut manager = LocaleManager {
            bundles: HashMap::new(),
            current_locale: String::new(),
            fallback_locale: "en-US".to_string(),
        };

        // Load all available locales
        manager.load_locales(assets_path)?;

        // Determine which locale to use
        let target_locale = if let Some(locale) = preferred_locale {
            info!("Using specified locale: {}", locale);
            locale.to_string()
        } else {
            // Detect system locale
            let system_locale = Self::detect_system_locale();
            info!("Detected system locale: {}", system_locale);
            system_locale
        };

        // Set current locale (fallback to en-US if not available)
        if manager.bundles.contains_key(&target_locale) {
            manager.current_locale = target_locale;
        } else {
            warn!("Locale {} not available, falling back to {}", target_locale, manager.fallback_locale);
            manager.current_locale = manager.fallback_locale.clone();
        }

        info!("Active locale: {}", manager.current_locale);

        Ok(manager)
    }

    /// Load all locale files from assets/locales directory
    fn load_locales(&mut self, assets_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let locales_path = Path::new(assets_path).join("locales");

        if !locales_path.exists() {
            return Err(format!("Locales directory not found: {:?}", locales_path).into());
        }

        // Iterate through locale directories
        for entry in fs::read_dir(&locales_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(locale_name) = path.file_name().and_then(|n| n.to_str()) {
                    match self.load_locale(locale_name, &path) {
                        Ok(_) => info!("Loaded locale: {}", locale_name),
                        Err(e) => warn!("Failed to load locale {}: {}", locale_name, e),
                    }
                }
            }
        }

        if self.bundles.is_empty() {
            return Err("No locales loaded".into());
        }

        Ok(())
    }

    /// Load a single locale from its directory
    fn load_locale(&mut self, locale_name: &str, locale_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Parse language identifier
        let langid: LanguageIdentifier = locale_name.parse()?;

        // Create bundle
        let mut bundle = FluentBundle::new(vec![langid]);

        // Load main.ftl file
        let main_file = locale_path.join("main.ftl");
        if main_file.exists() {
            let ftl_string = fs::read_to_string(&main_file)?;
            let resource = FluentResource::try_new(ftl_string)
                .map_err(|e| format!("Failed to parse FTL: {:?}", e))?;
            bundle.add_resource(resource)
                .map_err(|e| format!("Failed to add resource: {:?}", e))?;
        } else {
            return Err(format!("main.ftl not found in {:?}", locale_path).into());
        }

        self.bundles.insert(locale_name.to_string(), bundle);
        Ok(())
    }

    /// Detect system locale using sys-locale crate
    fn detect_system_locale() -> String {
        sys_locale::get_locale()
            .unwrap_or_else(|| "en-US".to_string())
    }

    /// Get a localized message by ID
    pub fn get(&self, id: &str) -> String {
        self.get_with_args(id, None)
    }

    /// Get a localized message with arguments
    pub fn get_with_args(&self, id: &str, args: Option<&FluentArgs>) -> String {
        // Try current locale first
        if let Some(bundle) = self.bundles.get(&self.current_locale) {
            if let Some(message) = bundle.get_message(id) {
                if let Some(pattern) = message.value() {
                    let mut errors = vec![];
                    let value = bundle.format_pattern(pattern, args, &mut errors);

                    if !errors.is_empty() {
                        warn!("Fluent formatting errors for '{}': {:?}", id, errors);
                    }

                    return value.to_string();
                }
            }
        }

        // Fallback to default locale
        if self.current_locale != self.fallback_locale {
            if let Some(bundle) = self.bundles.get(&self.fallback_locale) {
                if let Some(message) = bundle.get_message(id) {
                    if let Some(pattern) = message.value() {
                        let mut errors = vec![];
                        let value = bundle.format_pattern(pattern, args, &mut errors);
                        return value.to_string();
                    }
                }
            }
        }

        // If all else fails, return the ID itself
        error!("Message not found: {}", id);
        format!("[{}]", id)
    }

    /// Get current locale
    pub fn current_locale(&self) -> &str {
        &self.current_locale
    }

    /// Set current locale
    pub fn set_locale(&mut self, locale: &str) -> Result<(), String> {
        if self.bundles.contains_key(locale) {
            self.current_locale = locale.to_string();
            info!("Locale changed to: {}", locale);
            Ok(())
        } else {
            Err(format!("Locale not available: {}", locale))
        }
    }

    /// Get list of available locales
    pub fn available_locales(&self) -> Vec<String> {
        self.bundles.keys().cloned().collect()
    }
}

/// Helper macro for creating FluentArgs
#[macro_export]
macro_rules! fluent_args {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut args = fluent_bundle::FluentArgs::new();
        $(
            args.set($key, fluent_bundle::FluentValue::from($value.to_string()));
        )*
        args
    }};
}
