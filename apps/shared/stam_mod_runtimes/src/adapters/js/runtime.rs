use rquickjs::loader::{Loader, ModuleLoader, Resolver};
use rquickjs::{AsyncContext, AsyncRuntime, Ctx, Function, Module, Object, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tracing::{debug, error, trace};

/// Registry to track already-logged promise rejections to avoid duplicates
/// QuickJS calls the rejection tracker multiple times for the same rejection
static LOGGED_REJECTIONS: std::sync::LazyLock<Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));

/// Flag to indicate a fatal JavaScript error occurred
/// When set to true, the event loop should terminate and the client should exit
static JS_FATAL_ERROR: AtomicBool = AtomicBool::new(false);

/// Notify handle to wake up the event loop when a fatal error occurs
static FATAL_ERROR_NOTIFY: std::sync::LazyLock<tokio::sync::Notify> =
    std::sync::LazyLock::new(|| tokio::sync::Notify::new());

/// Check if a fatal JavaScript error has occurred
pub fn has_fatal_error() -> bool {
    JS_FATAL_ERROR.load(Ordering::SeqCst)
}

/// Reset the fatal error flag (call when starting a new session)
pub fn reset_fatal_error() {
    JS_FATAL_ERROR.store(false, Ordering::SeqCst);
}

/// Signal that a fatal error has occurred and wake the event loop
fn signal_fatal_error() {
    JS_FATAL_ERROR.store(true, Ordering::SeqCst);
    FATAL_ERROR_NOTIFY.notify_one();
}

use super::{JsRuntimeConfig, bindings};
use crate::api::{AppApi, LocaleApi, NetworkApi, SystemApi, ModInfo, UriResponse};
use crate::{ModReturnValue, RuntimeAdapter};
use bindings::TempFileManager;

/// Format a Promise rejection reason into a readable error message
///
/// Extracts error name, message, and stack trace from JavaScript Error objects.
/// Also attempts to read the source code line that caused the error.
fn format_rejection_reason(_ctx: &Ctx, reason: &Value) -> String {
    // Try to convert to object to access Error properties
    if let Some(obj) = reason.as_object() {
        let mut error_name = String::from("Error");
        let mut error_message = String::new();
        let mut stack_trace = String::new();

        // Try to get error name (e.g., "TypeError", "ReferenceError")
        if let Ok(name_prop) = obj.get::<_, Value>("name") {
            if let Some(name_str) = name_prop.as_string() {
                if let Ok(name) = name_str.to_string() {
                    error_name = name;
                }
            }
        }

        // Try to get error message
        if let Ok(msg_prop) = obj.get::<_, Value>("message") {
            if let Some(msg_str) = msg_prop.as_string() {
                if let Ok(msg) = msg_str.to_string() {
                    error_message = msg;
                }
            }
        }

        // Try to get stack trace
        if let Ok(stack_prop) = obj.get::<_, Value>("stack") {
            if let Some(stack_str) = stack_prop.as_string() {
                if let Ok(stack) = stack_str.to_string() {
                    stack_trace = stack;
                }
            }
        }

        // Format output
        let mut output = if !error_message.is_empty() {
            format!("{}: {}", error_name, error_message)
        } else {
            error_name
        };

        // Try to extract source code context from the first stack frame
        if let Some(source_context) = extract_source_context(&stack_trace) {
            output.push_str("\n  > ");
            output.push_str(&source_context);
        }

        // Add stack trace if available and not already included
        if !stack_trace.is_empty() && !output.contains(&stack_trace) {
            output.push('\n');
            output.push_str(&stack_trace);
        }

        return output;
    }

    // Fallback: try to convert directly to string
    if let Some(s) = reason.as_string() {
        if let Ok(msg) = s.to_string() {
            return msg;
        }
    }

    // Last resort: debug format
    format!("{:?}", reason)
}

/// Normalize a path by resolving `.` and `..` components without requiring the path to exist
fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};

    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {
                // Skip `.` components
            }
            Component::ParentDir => {
                // Go up one directory if possible
                normalized.pop();
            }
            _ => {
                normalized.push(component);
            }
        }
    }

    normalized
}

/// Parse URI components: host, path, and optional query string
///
/// Input format: "scheme://host:port/path?query" or "scheme://host/path"
/// Returns: (host, path, Option<query>)
///
/// Examples:
/// - "stam://localhost:9999/mods-manager/test/download" -> ("localhost:9999", "/mods-manager/test/download", None)
/// - "http://example.com/api?foo=bar" -> ("example.com", "/api", Some("foo=bar"))
fn parse_uri_components(uri: &str) -> (String, String, Option<String>) {
    // Find scheme separator
    let after_scheme = if let Some(pos) = uri.find("://") {
        &uri[pos + 3..]
    } else {
        uri
    };

    // Split host from path
    let (host, path_and_query) = if let Some(slash_pos) = after_scheme.find('/') {
        (&after_scheme[..slash_pos], &after_scheme[slash_pos..])
    } else {
        (after_scheme, "/")
    };

    // Split path from query
    let (path, query) = if let Some(query_pos) = path_and_query.find('?') {
        (
            &path_and_query[..query_pos],
            Some(path_and_query[query_pos + 1..].to_string()),
        )
    } else {
        (path_and_query, None)
    };

    (host.to_string(), path.to_string(), query)
}

/// Extract source code context from the first line of a stack trace
///
/// Parses stack trace lines like:
///   "at ensure_mods (/path/to/file.js:31:13)"
/// And reads line 31 from the file to show the problematic code.
fn extract_source_context(stack_trace: &str) -> Option<String> {
    // Find first "at " line which contains file:line:col
    for line in stack_trace.lines() {
        let line = line.trim();
        if line.starts_with("at ") {
            // Parse: "at funcName (file:line:col)" or "at file:line:col"
            if let Some(paren_start) = line.find('(') {
                if let Some(paren_end) = line.rfind(')') {
                    let location = &line[paren_start + 1..paren_end];
                    return read_source_line(location);
                }
            } else {
                // Format: "at file:line:col"
                let location = &line[3..]; // Skip "at "
                return read_source_line(location);
            }
        }
    }
    None
}

/// Read a specific line from a source file
///
/// Input format: "/path/to/file.js:31:13" (file:line:col)
fn read_source_line(location: &str) -> Option<String> {
    // Parse file:line:col - find last two colons
    let parts: Vec<&str> = location.rsplitn(3, ':').collect();
    if parts.len() < 3 {
        return None;
    }

    let _col: usize = parts[0].parse().ok()?;
    let line_num: usize = parts[1].parse().ok()?;
    let file_path = parts[2];

    // Read the file and get the specific line
    let content = fs::read_to_string(file_path).ok()?;
    let line = content.lines().nth(line_num.saturating_sub(1))?;

    Some(line.trim().to_string())
}

/// Global registry mapping mod aliases (@mod-id) to their entry point paths
///
/// This is shared across all JavaScript runtime instances and allows mods
/// to import other mods using the `@mod-id` syntax, e.g.:
/// ```javascript
/// import { Manager } from "@js-helper";
/// ```
static MOD_ALIAS_REGISTRY: std::sync::LazyLock<RwLock<HashMap<String, PathBuf>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register a mod alias for cross-mod imports
///
/// # Arguments
/// * `mod_id` - The mod identifier (will be accessible as `@mod_id`)
/// * `entry_point` - The absolute path to the mod's entry point file
pub fn register_mod_alias(mod_id: &str, entry_point: PathBuf) {
    let alias = format!("@{}", mod_id);
    //debug!("Registering mod alias: {} -> {}", alias, entry_point.display());
    let mut registry = MOD_ALIAS_REGISTRY.write().unwrap();
    registry.insert(alias, entry_point);
}

/// Custom resolver that handles @mod-id imports
///
/// This resolver intercepts imports starting with `@` and resolves them
/// to the registered mod's entry point path. For all other imports,
/// it delegates to the standard file resolution.
#[derive(Clone)]
struct ModAliasResolver;

impl Resolver for ModAliasResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
    ) -> rquickjs::Result<String> {
        //debug!("ModAliasResolver: resolve called with base='{}', name='{}'", base, name);

        // Check if this is a mod alias import (@mod-id)
        if name.starts_with('@') {
            let registry = MOD_ALIAS_REGISTRY.read().unwrap();

            // The alias might be just "@mod-id" or "@mod-id/subpath"
            let (alias, subpath) = if let Some(slash_pos) = name[1..].find('/') {
                let alias_end = slash_pos + 1;
                (&name[..alias_end], Some(&name[alias_end + 1..]))
            } else {
                (name, None)
            };

            if let Some(entry_point) = registry.get(alias) {
                let resolved = if let Some(sub) = subpath {
                    // @mod-id/subpath -> mod_dir/subpath
                    let mod_dir = entry_point.parent().unwrap_or(Path::new("."));
                    let joined = mod_dir.join(sub);
                    // Normalize the path to remove ./ and .. components
                    normalize_path(&joined).to_string_lossy().to_string()
                } else {
                    // @mod-id -> mod's entry point (already normalized via canonicalize)
                    entry_point.to_string_lossy().to_string()
                };

                //debug!("ModAliasResolver: {} -> {}", name, resolved);
                return Ok(resolved);
            } else {
                error!(
                    "ModAliasResolver: Unknown mod alias '{}'. Available aliases: {:?}",
                    alias,
                    registry.keys().collect::<Vec<_>>()
                );
                return Err(rquickjs::Error::new_resolving(base, name));
            }
        }

        // For relative imports, resolve relative to base
        if name.starts_with('.') {
            let base_dir = Path::new(base).parent().unwrap_or(Path::new("."));
            let resolved = base_dir.join(name);
            // Normalize the path to remove ./ and .. components
            let normalized = normalize_path(&resolved);
            let resolved_str = normalized.to_string_lossy().to_string();
            //debug!("ModAliasResolver: relative '{}' (from '{}') -> '{}'", name, base, resolved_str);
            return Ok(resolved_str);
        }

        // For absolute or other imports, return as-is
        //debug!("ModAliasResolver: passthrough '{}'", name);
        Ok(name.to_string())
    }
}

/// Custom filesystem loader for JavaScript modules
struct FilesystemLoader;

impl Loader for FilesystemLoader {
    fn load<'js>(&mut self, ctx: &rquickjs::Ctx<'js>, path: &str) -> rquickjs::Result<Module<'js>> {
        //debug!("FilesystemLoader: Loading module from path: '{}'", path);

        // Try to read the file, with automatic .js extension fallback
        let (actual_path, content) = Self::read_with_js_fallback(path)?;

        trace!(
            "FilesystemLoader: Successfully read {} bytes from '{}'",
            content.len(),
            actual_path
        );

        // Compile and return the module using the actual path found
        Module::declare(ctx.clone(), actual_path.clone(), content).map_err(|e| {
            error!(
                "FilesystemLoader: Failed to declare module '{}': {:?}",
                actual_path, e
            );
            e
        })
    }
}

impl FilesystemLoader {
    /// Try to read a file, automatically adding .js extension if needed
    fn read_with_js_fallback(path: &str) -> rquickjs::Result<(String, String)> {
        // First, try the exact path
        if let Ok(content) = fs::read_to_string(path) {
            return Ok((path.to_string(), content));
        }

        // If the path doesn't have a .js extension, try adding it
        if !path.ends_with(".js") {
            let path_with_js = format!("{}.js", path);
            if let Ok(content) = fs::read_to_string(&path_with_js) {
                //debug!("FilesystemLoader: Resolved '{}' to '{}'", path, path_with_js);
                return Ok((path_with_js, content));
            }

            // Also try index.js in case it's a directory import
            let index_path = format!("{}/index.js", path);
            if let Ok(content) = fs::read_to_string(&index_path) {
                //debug!("FilesystemLoader: Resolved '{}' to '{}'", path, index_path);
                return Ok((index_path, content));
            }
        }

        // Nothing worked, return error with original path
        error!(
            "FilesystemLoader: Failed to read file '{}' (also tried .js extension)",
            path
        );
        Err(rquickjs::Error::new_loading(path))
    }
}

/// Represents a loaded mod with its own isolated context
struct LoadedMod {
    context: AsyncContext,
    #[allow(dead_code)]
    mod_dir: PathBuf,
}

/// JavaScript runtime adapter for QuickJS with async support
///
/// Each mod gets its own isolated Context to prevent interference between mods.
/// All contexts share the same Runtime with a shared module loader.
pub struct JsRuntimeAdapter {
    runtime: Arc<AsyncRuntime>,
    config: JsRuntimeConfig,
    /// Map of mod_id to loaded mod instance
    loaded_mods: HashMap<String, LoadedMod>,
    /// Collection of all mod directories for module resolution
    mod_dirs: Vec<PathBuf>,
    /// System API shared across all mod contexts
    system_api: SystemApi,
    /// Locale API for internationalization (optional)
    locale_api: Option<LocaleApi>,
    /// Network API for downloading resources (optional, client-side only)
    network_api: Option<NetworkApi>,
    /// Graphic proxy for graphic engine operations (optional, client-side only)
    graphic_proxy: Option<Arc<crate::api::GraphicProxy>>,
    /// Resource proxy for resource loading and caching (optional, client-side only)
    resource_proxy: Option<Arc<crate::api::ResourceProxy>>,
    /// Temp file manager for downloaded content (tracks and cleans up temp files)
    temp_file_manager: TempFileManager,
}

impl JsRuntimeAdapter {
    /// Create a new JavaScript runtime adapter with QuickJS async support
    ///
    /// # Arguments
    /// * `config` - Runtime configuration containing game directories
    pub fn new(config: JsRuntimeConfig) -> Result<Self, Box<dyn std::error::Error>> {
        debug!("> Initializing javascript async runtime \"QuickJS\" for mods");

        let runtime = AsyncRuntime::new()?;

        // Setup promise rejection tracker synchronously using block_on
        // This must be done before any JavaScript code runs
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                runtime.set_host_promise_rejection_tracker(Some(Box::new(
                    |ctx, _promise, reason, is_handled| {
                        // Only report unhandled rejections (is_handled == false)
                        if !is_handled {
                            // Try to extract error message from the reason
                            let error_msg = format_rejection_reason(&ctx, &reason);

                            // Get mod ID for logging context
                            let mod_id: String = ctx
                                .globals()
                                .get("__MOD_ID__")
                                .unwrap_or_else(|_| "unknown".to_string());

                            // Create a unique key for this rejection to avoid duplicates
                            // QuickJS may call the tracker multiple times for the same rejection
                            let rejection_key = format!("{}:{}", mod_id, error_msg);

                            // Check if we've already logged this rejection
                            let mut logged = LOGGED_REJECTIONS.lock().unwrap();
                            if logged.contains(&rejection_key) {
                                return;
                            }
                            logged.insert(rejection_key);

                            // Limit the set size to prevent unbounded growth
                            if logged.len() > 100 {
                                logged.clear();
                            }

                            error!("js::{}: Unhandled Promise Rejection: {}", mod_id, error_msg);

                            // Signal fatal error and wake the event loop
                            signal_fatal_error();
                        }
                    },
                ))).await;
            });
        });

        let js_runtime = Self {
            runtime: Arc::new(runtime),
            config,
            loaded_mods: HashMap::new(),
            mod_dirs: Vec::new(),
            system_api: SystemApi::new(),
            locale_api: None,
            network_api: None,
            graphic_proxy: None,
            resource_proxy: None,
            temp_file_manager: TempFileManager::new(),
        };

        debug!("< JavaScript async runtime \"QuickJS\" initialized successfully");
        Ok(js_runtime)
    }

    /// Set the locale API for internationalization support
    ///
    /// This should be called before loading any mods to ensure
    /// the `locale` global object is available in all mod contexts.
    pub fn set_locale_api(&mut self, locale_api: LocaleApi) {
        self.locale_api = Some(locale_api);
    }

    /// Set the network API for network operations
    ///
    /// This should be called before loading any mods to ensure
    /// the `network` global object is available in all mod contexts.
    /// Typically only used on the client side.
    pub fn set_network_api(&mut self, network_api: NetworkApi) {
        self.network_api = Some(network_api);
    }

    /// Set the graphic proxy for graphic engine operations
    ///
    /// This should be called before loading any mods to ensure
    /// the `graphic` global object is available in all mod contexts.
    /// Only used on the client side.
    pub fn set_graphic_proxy(&mut self, graphic_proxy: Arc<crate::api::GraphicProxy>) {
        self.graphic_proxy = Some(graphic_proxy);
    }

    /// Set the resource proxy for resource loading and caching
    ///
    /// This should be called before loading any mods to ensure
    /// the `Resource` global object is available in all mod contexts.
    /// Only used on the client side.
    pub fn set_resource_proxy(&mut self, resource_proxy: Arc<crate::api::ResourceProxy>) {
        self.resource_proxy = Some(resource_proxy);
    }

    /// Get a clone of the async runtime for the event loop
    pub fn get_runtime(&self) -> Arc<AsyncRuntime> {
        Arc::clone(&self.runtime)
    }

    /// Register a mod with the system API
    ///
    /// This makes the mod visible to `system.get_mods()` calls.
    /// Should be called after loading the mod's manifest.
    pub fn register_mod_info(&self, mod_info: ModInfo) {
        self.system_api.register_mod(mod_info);
    }

    /// Mark a mod as bootstrapped
    ///
    /// Should be called after `onBootstrap` is invoked for a mod.
    pub fn set_mod_bootstrapped(&self, mod_id: &str, bootstrapped: bool) {
        self.system_api.set_bootstrapped(mod_id, bootstrapped);
    }

    /// Get a reference to the system API
    pub fn system_api(&self) -> &SystemApi {
        &self.system_api
    }

    /// Setup all global APIs in a context
    async fn setup_global_apis(
        &self,
        context: &AsyncContext,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let game_data_dir = self.config.game_data_dir().clone();
        let game_config_dir = self.config.game_config_dir().clone();
        let system_api = self.system_api.clone();
        let locale_api = self.locale_api.clone();
        let network_api = self.network_api.clone();
        let graphic_proxy = self.graphic_proxy.clone();
        let resource_proxy = self.resource_proxy.clone();
        let temp_file_manager = self.temp_file_manager.clone();

        // Configure temp directory for downloads (game_data_dir/tmp)
        let temp_dir = game_data_dir.join("tmp");
        temp_file_manager.set_temp_dir(temp_dir);

        trace!(
            "setup_global_apis: game_data_dir={:?}, game_config_dir={:?}",
            game_data_dir, game_config_dir
        );

        context
            .with(|ctx| {
                // Register console API
                //debug!("Begin API registrations...");
                bindings::setup_console_api(ctx.clone())?;

                // Register process API with game-specific directories
                let app_api = AppApi::new(game_data_dir.clone(), game_config_dir.clone());
                bindings::setup_process_api(ctx.clone(), app_api)?;

                // Register file API with game-specific directories for path validation
                let file_api = crate::api::FileApi::new(game_data_dir, game_config_dir.clone());
                bindings::setup_file_api(ctx.clone(), file_api)?;

                // Register timer API (setTimeout, setInterval, etc.)
                bindings::setup_timer_api(ctx.clone())?;

                // Register system API (system.get_mods(), system.getGameConfigPath())
                // game_config_dir is passed for client-only getGameConfigPath() method
                let config_dir_for_system = if game_config_dir.as_os_str().is_empty() {
                    None
                } else {
                    Some(game_config_dir)
                };
                // Clone system_api before moving it - we need it later for Resource API
                let system_api_for_resource = system_api.clone();
                bindings::setup_system_api(ctx.clone(), system_api, config_dir_for_system)?;

                // Register locale API (locale.get(), locale.get_with_args())
                if let Some(locale) = locale_api {
                    bindings::setup_locale_api(ctx.clone(), locale)?;
                }

                // Register network API (network.download()) - client-side only
                if let Some(network) = network_api {
                    bindings::setup_network_api(ctx.clone(), network, temp_file_manager)?;
                }

                // Register graphic API (graphic.enableEngine(), etc.) - client-side only
                if let Some(proxy) = graphic_proxy.clone() {
                    bindings::setup_graphic_api(ctx.clone(), proxy)?;
                }

                // Register resource API (Resource.load(), Resource.unload(), etc.) - client-side only
                // Requires both resource_proxy and graphic_proxy to be set
                if let (Some(res_proxy), Some(gfx_proxy)) = (resource_proxy, graphic_proxy) {
                    bindings::setup_resource_api(ctx.clone(), res_proxy, gfx_proxy, system_api_for_resource)?;
                }

                // Register text API (Text.DecodeUTF8())
                bindings::setup_text_api(ctx.clone())?;

                //debug!(" > API registrations completed successfully");
                Ok::<(), rquickjs::Error>(())
            })
            .await?;

        Ok(())
    }

    /// Format a JavaScript error with stack trace in Node.js style
    fn format_js_error(ctx: &rquickjs::Ctx, _error: &rquickjs::Error) -> String {
        use rquickjs::Value;

        let mut output = String::new();

        // Get the exception value
        let exception = ctx.catch();

        // Try to convert to object to access properties
        if let Some(obj) = exception.as_object() {
            let mut error_name = String::from("Error");
            let mut error_message = String::new();
            let mut stack_trace = String::new();

            // Try to get error name (e.g., "TypeError", "ReferenceError")
            if let Ok(name_prop) = obj.get::<_, Value>("name") {
                if let Some(name_str) = name_prop.as_string() {
                    if let Ok(name) = name_str.to_string() {
                        error_name = name;
                    }
                }
            }

            // Try to get error message
            if let Ok(msg_prop) = obj.get::<_, Value>("message") {
                if let Some(msg_str) = msg_prop.as_string() {
                    if let Ok(msg) = msg_str.to_string() {
                        error_message = msg;
                    }
                }
            }

            // Try to get stack trace
            if let Ok(stack_prop) = obj.get::<_, Value>("stack") {
                if let Some(stack_str) = stack_prop.as_string() {
                    if let Ok(stack) = stack_str.to_string() {
                        stack_trace = stack;
                    }
                }
            }

            // Format output: "Error: message\n    at ..."
            if !error_message.is_empty() {
                output.push_str(&format!("{}: {}", error_name, error_message));
            } else {
                output.push_str(&error_name);
            }

            // Add stack trace if available
            if !stack_trace.is_empty() {
                if !stack_trace.starts_with(&error_name) {
                    output.push('\n');
                    output.push_str(&stack_trace);
                } else {
                    output = stack_trace;
                }
            }

            return output;
        }

        // Fallback: try to convert exception directly to string
        if let Some(s) = exception.as_string() {
            if let Ok(msg) = s.to_string() {
                output.push_str(&format!("Error: {}", msg));
                return output;
            }
        }

        // Try to get more debug info from the rquickjs::Error
        output.push_str(&format!("Error: Unknown JavaScript error (rquickjs: {:?})", _error));
        output
    }

    /// Load a mod asynchronously
    pub async fn load_mod_async(
        &mut self,
        mod_path: &Path,
        mod_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        trace!(
            "Loading JavaScript module: {} from {}",
            mod_id,
            mod_path.display()
        );

        // Get the mod directory (parent of the entry point file)
        let mod_dir = mod_path
            .parent()
            .ok_or_else(|| {
                format!(
                    "Cannot determine mod directory for '{}'",
                    mod_path.display()
                )
            })?
            .to_path_buf();

        // Load mod-specific locales if the mod has a locale/ directory
        if let Some(ref locale_api) = self.locale_api {
            if let Err(e) = locale_api.load_mod_locales(mod_id, &mod_dir) {
                // Log warning but don't fail - mod can still work without custom locales
                tracing::warn!("Failed to load locales for mod '{}': {}", mod_id, e);
            }
        }

        // Register mod alias for cross-mod imports (@mod-id syntax)
        // Use absolute path for reliable resolution - canonicalize to remove ./ and normalize
        let absolute_entry_point = fs::canonicalize(mod_path).map_err(|e| {
            format!(
                "Failed to canonicalize path '{}': {}",
                mod_path.display(),
                e
            )
        })?;
        register_mod_alias(mod_id, absolute_entry_point.clone());

        // Add mod directory to the list of search paths and update loader
        if !self.mod_dirs.contains(&mod_dir) {
            self.mod_dirs.push(mod_dir.clone());

            // Use ModAliasResolver for @mod-id imports, combined with FileResolver for relative imports
            let resolver = ModAliasResolver;

            let loader = (FilesystemLoader, ModuleLoader::default());
            self.runtime.set_loader(resolver, loader).await;
        }

        // Create a new isolated AsyncContext for this mod
        let context = AsyncContext::full(&self.runtime).await?;

        // Setup global APIs for this mod's context
        self.setup_global_apis(&context).await?;

        // Set global __GAME_ID__ (optional) and __MOD_ID__ variables for console logging
        let game_id = self.config.game_id().map(|s| s.to_string());
        context
            .with(|ctx| {
                if let Some(gid) = game_id {
                    ctx.globals().set("__GAME_ID__", gid)?;
                }
                ctx.globals().set("__MOD_ID__", mod_id)?;
                Ok::<(), rquickjs::Error>(())
            })
            .await?;

        // Use absolute path for the initial module import
        // This ensures the loader can find the file regardless of working directory
        let module_path_str = absolute_entry_point.to_string_lossy().to_string();

        let mod_id_owned = mod_id.to_string();

        // Read the entry point file content
        let entry_content = fs::read_to_string(&absolute_entry_point)
            .map_err(|e| format!("Failed to read entry point '{}': {}", module_path_str, e))?;

        // Load the module from the filesystem
        // Use Result<String, String> for ParallelSend compatibility
        let result: Result<String, String> = context
            .with(|ctx| {
                // Declare the module with the file content
                match Module::declare(ctx.clone(), module_path_str.clone(), entry_content) {
                    Ok(module) => {
                        // Evaluate the module to execute it
                        match module.eval() {
                            Ok((evaluated_module, promise)) => match promise.finish::<()>() {
                                Ok(_) => {
                                    // Store the module namespace in a global variable for later access
                                    // This avoids re-importing the module
                                    let namespace_key =
                                        format!("__MODULE_NS_{}__", mod_id_owned.replace("-", "_"));
                                    if let Ok(namespace) = evaluated_module.namespace() {
                                        if let Err(e) = ctx.globals().set(&namespace_key, namespace)
                                        {
                                            error!("Failed to store module namespace: {:?}", e);
                                        }
                                    }
                                    Ok(module_path_str.clone())
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("{}", error_msg);
                                    Err(format!("JavaScript error in mod '{}'", mod_id_owned))
                                }
                            },
                            Err(e) => {
                                let error_msg = Self::format_js_error(&ctx, &e);
                                error!("{}", error_msg);
                                Err(format!(
                                    "JavaScript error evaluating mod '{}'",
                                    mod_id_owned
                                ))
                            }
                        }
                    }
                    Err(e) => {
                        let error_msg = Self::format_js_error(&ctx, &e);
                        error!("{}", error_msg);
                        Err(format!("JavaScript error declaring mod '{}'", mod_id_owned))
                    }
                }
            })
            .await;

        let stored_module_path = result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

        // Store the loaded mod
        let _ = stored_module_path; // Used for logging/debugging if needed
        self.loaded_mods.insert(
            mod_id.to_string(),
            LoadedMod {
                context,
                mod_dir,
            },
        );

        Ok(())
    }

    /// Call a mod function asynchronously
    pub async fn call_mod_function_async(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        //debug!("Calling JavaScript function '{}' for mod '{}'", function_name, mod_id);

        let loaded_mod = self
            .loaded_mods
            .get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        let mod_id_owned = mod_id.to_string();
        let function_name_owned = function_name.to_string();

        // Use Result<(), String> for ParallelSend compatibility
        // Get the stored module namespace from globals instead of re-importing
        let namespace_key = format!("__MODULE_NS_{}__", mod_id.replace("-", "_"));

        let result: Result<(), String> = loaded_mod
            .context
            .with(|ctx| {
                // Get the stored module namespace from globals
                match ctx.globals().get::<_, Object>(&namespace_key) {
                    Ok(module_namespace) => {
                        match module_namespace.get::<_, rquickjs::Function>(&function_name_owned) {
                            Ok(func) => {
                                // Call function and get result as Value to check if it's a Promise
                                match func.call::<(), Value>(()) {
                                    Ok(result) => {
                                        // Check if result is a Promise
                                        if let Some(promise) = result.clone().into_promise() {
                                            // It's a Promise - we need to resolve it
                                            match promise.finish::<()>() {
                                                Ok(_) => {
                                                    //debug!("Async function '{}' resolved successfully for mod '{}'", function_name_owned, mod_id_owned);
                                                    Ok(())
                                                }
                                                Err(e) => {
                                                    let error_msg = Self::format_js_error(&ctx, &e);
                                                    error!("{}", error_msg);
                                                    Err(format!(
                                                        "JavaScript error in async '{}' for mod '{}'",
                                                        function_name_owned, mod_id_owned
                                                    ))
                                                }
                                            }
                                        } else {
                                            // Not a Promise, just return success
                                            //debug!("Function '{}' executed successfully for mod '{}'", function_name_owned, mod_id_owned);
                                            Ok(())
                                        }
                                    }
                                    Err(e) => {
                                        let error_msg = Self::format_js_error(&ctx, &e);
                                        error!("{}", error_msg);
                                        Err(format!(
                                            "JavaScript error in '{}' for mod '{}'",
                                            function_name_owned, mod_id_owned
                                        ))
                                    }
                                }
                            }
                            Err(_) => {
                                debug!(
                                    "Function '{}' not found or not exported for mod '{}'",
                                    function_name_owned, mod_id_owned
                                );
                                Ok(())
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to get module namespace '{}': {:?}",
                            namespace_key, e
                        );
                        Err(format!(
                            "Failed to get module namespace for mod '{}'",
                            mod_id_owned
                        ))
                    }
                }
            })
            .await;

        result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    }

    /// Dispatch a RequestUri event to all registered handlers
    ///
    /// This method finds all handlers registered for the given URI, calls them
    /// in priority order (lowest first), and returns the final UriResponse.
    ///
    /// # Arguments
    /// * `uri` - The URI being requested
    ///
    /// # Returns
    /// A `UriResponse` containing the result of handler processing
    pub async fn dispatch_request_uri(
        &self,
        uri: &str,
    ) -> UriResponse {
        // Log total number of RequestUri handlers registered
        // let total_handlers = self.system_api.event_dispatcher().handler_count(crate::api::SystemEvents::RequestUri);
        //info!("dispatch_request_uri: uri='{}', total_handlers={}", uri, total_handlers);

        let handlers = self.system_api.event_dispatcher().get_handlers_for_uri_request(uri);

        if handlers.is_empty() {
            //info!("No handlers matched for URI: {} (total registered: {})", uri, total_handlers);
            return UriResponse::default();
        }

        //info!("Dispatching RequestUri to {} handlers for URI: {}", handlers.len(), uri);

        let mut response = UriResponse::default();
        let uri_owned = uri.to_string();

        for handler in handlers {
            // Get the mod's context
            let loaded_mod = match self.loaded_mods.get(&handler.mod_id) {
                Some(m) => m,
                None => {
                    error!("Handler mod '{}' not loaded", handler.mod_id);
                    continue;
                }
            };

            let handler_id = handler.handler_id;
            let mod_id = handler.mod_id.clone();
            let uri_for_closure = uri_owned.clone();

            // Parse URI components for the request object
            let (host, path, query) = parse_uri_components(&uri_for_closure);

            // Call the handler function with request and response objects
            let result: Result<(u16, bool, String, String), String> = loaded_mod
                .context
                .with(|ctx| {
                    // Get the handler function from the context's handler map
                    match bindings::get_js_handler(&ctx, handler_id) {
                        Ok(Some(func)) => {
                            // Create request object with uri, path, host, query
                            let request = Object::new(ctx.clone()).map_err(|e| format!("Failed to create request object: {:?}", e))?;
                            request.set("uri", uri_for_closure.as_str()).map_err(|e| format!("Failed to set uri: {:?}", e))?;
                            request.set("path", path.as_str()).map_err(|e| format!("Failed to set path: {:?}", e))?;
                            request.set("host", host.as_str()).map_err(|e| format!("Failed to set host: {:?}", e))?;
                            if let Some(ref q) = query {
                                request.set("query", q.as_str()).map_err(|e| format!("Failed to set query: {:?}", e))?;
                            } else {
                                request.set("query", rquickjs::Null).map_err(|e| format!("Failed to set query: {:?}", e))?;
                            }

                            // Create response object with methods
                            let response_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create response object: {:?}", e))?;
                            response_obj.set("status", 404i32).map_err(|e| format!("Failed to set status: {:?}", e))?;
                            response_obj.set("handled", false).map_err(|e| format!("Failed to set handled: {:?}", e))?;
                            response_obj.set("buffer_string", "").map_err(|e| format!("Failed to set buffer_string: {:?}", e))?;
                            response_obj.set("filepath", "").map_err(|e| format!("Failed to set filepath: {:?}", e))?;

                            // Add setStatus method
                            let set_status = Function::new(ctx.clone(), |ctx: Ctx, status: i32| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentResponse")?;
                                this.set("status", status)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setStatus: {:?}", e))?;
                            response_obj.set("setStatus", set_status).map_err(|e| format!("Failed to set setStatus: {:?}", e))?;

                            // Add setFilepath method
                            let set_filepath = Function::new(ctx.clone(), |ctx: Ctx, path: String| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentResponse")?;
                                this.set("filepath", path)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setFilepath: {:?}", e))?;
                            response_obj.set("setFilepath", set_filepath).map_err(|e| format!("Failed to set setFilepath: {:?}", e))?;

                            // Add setHandled method
                            let set_handled = Function::new(ctx.clone(), |ctx: Ctx, handled: bool| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentResponse")?;
                                this.set("handled", handled)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setHandled: {:?}", e))?;
                            response_obj.set("setHandled", set_handled).map_err(|e| format!("Failed to set setHandled: {:?}", e))?;

                            // Add setBufferString method
                            let set_buffer_string = Function::new(ctx.clone(), |ctx: Ctx, buffer_string: String| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentResponse")?;
                                this.set("buffer_string", buffer_string)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setBufferString: {:?}", e))?;
                            response_obj.set("setBufferString", set_buffer_string).map_err(|e| format!("Failed to set setBufferString: {:?}", e))?;

                            // Store response object as global for method access
                            ctx.globals().set("__currentResponse", response_obj.clone()).map_err(|e| format!("Failed to set __currentResponse: {:?}", e))?;

                            // Call the handler function
                            let call_result = func.call::<(Object, Object), Value>((request, response_obj.clone()));

                            match call_result {
                                Ok(result) => {
                                    // If result is a Promise, try to resolve it
                                    if let Some(promise) = result.into_promise() {
                                        match promise.finish::<()>() {
                                            Ok(_) => {
                                                // Promise resolved successfully
                                            }
                                            Err(rquickjs::Error::WouldBlock) => {
                                                // Promise is still pending - async handler is running in background
                                                // This is expected for async handlers that perform I/O operations
                                                debug!("Handler in mod '{}' returned pending Promise (async operation in progress)", mod_id);
                                            }
                                            Err(e) => {
                                                let error_msg = Self::format_js_error(&ctx, &e);
                                                error!("Handler error in mod '{}': {}", mod_id, error_msg);
                                                return Err(format!("Handler error: {}", error_msg));
                                            }
                                        }
                                    }

                                    // Read back the response values
                                    let status: i32 = response_obj.get("status").unwrap_or(404);
                                    let handled: bool = response_obj.get("handled").unwrap_or(false);
                                    let filepath: String = response_obj.get("filepath").unwrap_or_default();

                                    // Read buffer_string if present
                                    let buffer_string: String = response_obj.get("buffer_string").unwrap_or_default();

                                    Ok((status as u16, handled, buffer_string, filepath))
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("Handler call error in mod '{}': {}", mod_id, error_msg);
                                    Err(format!("Handler call error: {}", error_msg))
                                }
                            }
                        }
                        Ok(None) => {
                            error!("Handler {} not found in mod '{}'", handler_id, mod_id);
                            Err(format!("Handler {} not found", handler_id))
                        }
                        Err(e) => {
                            error!("Failed to get handler {} from mod '{}': {:?}", handler_id, mod_id, e);
                            Err(format!("Failed to get handler: {:?}", e))
                        }
                    }
                })
                .await;

            // Update response based on handler result
            match result {
                Ok((status, handled, buffer_string, filepath)) => {
                    response.status = status;
                    response.handled = handled;
                    if !buffer_string.is_empty() {
                        response.buffer_string = buffer_string;
                    }
                    if !filepath.is_empty() {
                        response.filepath = filepath;
                    }

                    // If handler set handled=true, stop processing more handlers
                    if handled {
                        trace!("Handler in mod '{}' marked request as handled", handler.mod_id);
                        break;
                    }
                }
                Err(e) => {
                    error!("Handler execution failed: {}", e);
                    // Continue to next handler on error
                }
            }
        }

        response
    }

    /// Dispatch a TerminalKeyPressed event to all registered handlers
    ///
    /// This method finds all handlers registered for TerminalKeyPressed, calls them
    /// in priority order (lowest first), and returns whether the event was handled.
    ///
    /// # Arguments
    /// * `request` - The terminal key request containing key and modifier information
    ///
    /// # Returns
    /// A `TerminalKeyResponse` containing whether the event was handled
    pub async fn dispatch_terminal_key(
        &self,
        request: &crate::api::TerminalKeyRequest,
    ) -> crate::api::TerminalKeyResponse {
        let handlers = self.system_api.event_dispatcher().get_handlers_for_terminal_key();

        if handlers.is_empty() {
            return crate::api::TerminalKeyResponse::default();
        }

        debug!("Dispatching TerminalKeyPressed to {} handlers for key: {}", handlers.len(), request.combo);

        let mut response = crate::api::TerminalKeyResponse::default();

        for handler in handlers {
            // Get the mod's context
            let loaded_mod = match self.loaded_mods.get(&handler.mod_id) {
                Some(m) => m,
                None => {
                    error!("Handler mod '{}' not loaded", handler.mod_id);
                    continue;
                }
            };

            let handler_id = handler.handler_id;
            let mod_id = handler.mod_id.clone();
            let key = request.key.clone();
            let ctrl = request.ctrl;
            let alt = request.alt;
            let shift = request.shift;
            let meta = request.meta;
            let combo = request.combo.clone();

            // Step 1: Call the handler and detect if it returns a Promise
            let call_result: Result<bool, String> = loaded_mod
                .context
                .with(|ctx| {
                    // Get the handler function from the context's handler map
                    match bindings::get_js_handler(&ctx, handler_id) {
                        Ok(Some(func)) => {
                            // Create request object
                            let request_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create request object: {:?}", e))?;
                            request_obj.set("key", key.as_str()).map_err(|e| format!("Failed to set key: {:?}", e))?;
                            request_obj.set("ctrl", ctrl).map_err(|e| format!("Failed to set ctrl: {:?}", e))?;
                            request_obj.set("alt", alt).map_err(|e| format!("Failed to set alt: {:?}", e))?;
                            request_obj.set("shift", shift).map_err(|e| format!("Failed to set shift: {:?}", e))?;
                            request_obj.set("meta", meta).map_err(|e| format!("Failed to set meta: {:?}", e))?;
                            request_obj.set("combo", combo.as_str()).map_err(|e| format!("Failed to set combo: {:?}", e))?;

                            // Create response object
                            let response_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create response object: {:?}", e))?;
                            response_obj.set("handled", false).map_err(|e| format!("Failed to set handled: {:?}", e))?;

                            // Add setHandled method
                            let set_handled = Function::new(ctx.clone(), |ctx: Ctx, handled: bool| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentTerminalKeyResponse")?;
                                this.set("handled", handled)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setHandled: {:?}", e))?;
                            response_obj.set("setHandled", set_handled).map_err(|e| format!("Failed to set setHandled: {:?}", e))?;

                            // Store response object as global for method access and later retrieval
                            ctx.globals().set("__currentTerminalKeyResponse", response_obj.clone()).map_err(|e| format!("Failed to set __currentTerminalKeyResponse: {:?}", e))?;

                            // Call the handler function
                            let call_result = func.call::<(Object, Object), Value>((request_obj, response_obj));

                            match call_result {
                                Ok(result) => {
                                    // Return true if handler returned a Promise, false otherwise
                                    Ok(result.is_promise())
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("Handler call error in mod '{}': {}", mod_id, error_msg);
                                    Err(format!("Handler call error: {}", error_msg))
                                }
                            }
                        }
                        Ok(None) => {
                            error!("Handler {} not found in mod '{}'", handler_id, mod_id);
                            Err(format!("Handler {} not found", handler_id))
                        }
                        Err(e) => {
                            error!("Failed to get handler {} from mod '{}': {:?}", handler_id, mod_id, e);
                            Err(format!("Failed to get handler: {:?}", e))
                        }
                    }
                })
                .await;

            // Check if handler call succeeded
            let was_promise = match call_result {
                Ok(is_promise) => is_promise,
                Err(e) => {
                    error!("Handler error: {}", e);
                    continue;
                }
            };

            // Step 2: If it was a Promise, we do NOT call runtime.idle() here.
            // The main event loop will process pending JS jobs naturally via run_js_event_loop().
            // Calling idle() here could cause deadlocks if the handler calls sendEvent(),
            // because the sendEvent awaits a response that can't arrive until this function returns.
            //
            // Handlers that need async operations (like system.exit()) should set their
            // response values synchronously before any await points.
            if was_promise {
                trace!("TerminalKeyPressed handler returned Promise - async work will complete via event loop");
            }

            // Step 3: Read the response object
            let result: Result<bool, String> = loaded_mod
                .context
                .with(|ctx| {
                    // Get the response object from globals
                    let response_obj: Object = ctx.globals().get("__currentTerminalKeyResponse")
                        .map_err(|e| format!("Failed to get response object: {:?}", e))?;

                    // Read back the response values
                    let handled: bool = response_obj.get("handled").unwrap_or(false);
                    Ok(handled)
                })
                .await;

            // Update response based on handler result
            match result {
                Ok(handled) => {
                    response.handled = handled;

                    // If handler set handled=true, stop processing more handlers
                    if handled {
                        trace!("Handler in mod '{}' marked TerminalKeyPressed as handled", handler.mod_id);
                        break;
                    }
                }
                Err(e) => {
                    error!("Handler execution failed: {}", e);
                    // Continue to next handler on error
                }
            }
        }

        response
    }

    /// Call a mod function asynchronously with return value
    pub async fn call_mod_function_with_return_async(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        //debug!("Calling JavaScript function '{}' for mod '{}' with return", function_name, mod_id);

        let loaded_mod = self
            .loaded_mods
            .get(mod_id)
            .ok_or_else(|| format!("Mod '{}' not loaded", mod_id))?;

        let mod_id_owned = mod_id.to_string();
        let function_name_owned = function_name.to_string();

        // Get the stored module namespace from globals instead of re-importing
        let namespace_key = format!("__MODULE_NS_{}__", mod_id.replace("-", "_"));

        // Use Result<ModReturnValue, String> for ParallelSend compatibility
        let result: Result<ModReturnValue, String> = loaded_mod
            .context
            .with(|ctx| {
                // Get the stored module namespace from globals
                match ctx.globals().get::<_, Object>(&namespace_key) {
                    Ok(module_namespace) => {
                        match module_namespace.get::<_, rquickjs::Function>(&function_name_owned) {
                            Ok(func) => match func.call::<(), String>(()) {
                                Ok(value) => {
                                    debug!(
                                        "Function '{}' returned string for mod '{}'",
                                        function_name_owned, mod_id_owned
                                    );
                                    Ok(ModReturnValue::String(value))
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("{}", error_msg);
                                    Err(format!(
                                        "JavaScript error in '{}' for mod '{}'",
                                        function_name_owned, mod_id_owned
                                    ))
                                }
                            },
                            Err(_) => {
                                debug!(
                                    "Function '{}' not found or not exported for mod '{}'",
                                    function_name_owned, mod_id_owned
                                );
                                Ok(ModReturnValue::None)
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to get module namespace '{}': {:?}",
                            namespace_key, e
                        );
                        Err(format!(
                            "Failed to get module namespace for mod '{}'",
                            mod_id_owned
                        ))
                    }
                }
            })
            .await;

        result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })
    }

    /// Call an event handler asynchronously
    ///
    /// This method finds the handler by its ID and calls it with the event name and arguments.
    /// The handler function was stored in the context's handler map during registration.
    ///
    /// # Arguments
    /// * `handler_id` - The unique handler ID returned from registration
    /// * `event_name` - The name of the event being dispatched
    /// * `args` - JSON-serialized arguments to pass to the handler
    pub async fn call_event_handler_async(
        &self,
        handler_id: u64,
        event_name: &str,
        args: &[String],
    ) -> Result<(), Box<dyn std::error::Error>> {
        trace!("Calling event handler {} for event '{}'", handler_id, event_name);

        // We need to find which mod context has this handler
        // For now, iterate through all loaded mods (could be optimized with a handler->mod map)
        for (mod_id, loaded_mod) in &self.loaded_mods {
            let event_name_owned = event_name.to_string();
            let args_owned: Vec<String> = args.to_vec();

            let result: Result<bool, String> = loaded_mod
                .context
                .with(|ctx| {
                    // Try to get the handler function from this context
                    match bindings::get_js_handler(&ctx, handler_id) {
                        Ok(Some(func)) => {
                            // Found the handler! Call it with event_name and args

                            // Convert args to JavaScript values (parse JSON strings)
                            let js_args = rquickjs::Array::new(ctx.clone())
                                .map_err(|e| format!("Failed to create args array: {:?}", e))?;

                            for (i, arg) in args_owned.iter().enumerate() {
                                // Try to parse as JSON, otherwise use as string
                                let js_value: Value = ctx.json_parse(arg.clone())
                                    .unwrap_or_else(|_| {
                                        // If JSON parse fails, use as plain string
                                        rquickjs::String::from_str(ctx.clone(), arg)
                                            .map(|s| s.into())
                                            .unwrap_or(Value::new_undefined(ctx.clone()))
                                    });
                                js_args.set(i, js_value)
                                    .map_err(|e| format!("Failed to set arg {}: {:?}", i, e))?;
                            }

                            // Call the handler function with event_name and args array
                            let call_result = func.call::<(String, rquickjs::Array), Value>((event_name_owned.clone(), js_args));

                            match call_result {
                                Ok(result) => {
                                    // If result is a Promise, resolve it
                                    if let Some(promise) = result.into_promise() {
                                        if let Err(e) = promise.finish::<()>() {
                                            let error_msg = Self::format_js_error(&ctx, &e);
                                            error!("Event handler error: {}", error_msg);
                                            return Err(format!("Event handler error: {}", error_msg));
                                        }
                                    }
                                    Ok(true) // Found and called successfully
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("Event handler call error: {}", error_msg);
                                    Err(format!("Event handler call error: {}", error_msg))
                                }
                            }
                        }
                        Ok(None) => {
                            // Handler not found in this context, try next mod
                            Ok(false)
                        }
                        Err(e) => {
                            // Error getting handler, try next mod
                            error!("Error getting handler {} from mod '{}': {:?}", handler_id, mod_id, e);
                            Ok(false)
                        }
                    }
                })
                .await;

            match result {
                Ok(true) => {
                    // Handler was found and called successfully
                    return Ok(());
                }
                Ok(false) => {
                    // Handler not in this mod, continue searching
                    continue;
                }
                Err(e) => {
                    // Handler found but execution failed
                    return Err(e.into());
                }
            }
        }

        // Handler not found in any mod
        Err(format!("Event handler {} not found in any loaded mod", handler_id).into())
    }

    /// Cleanup temp files created during script execution
    ///
    /// This should be called when the runtime is shutting down to ensure
    /// all temporary files created by `network.download()` are removed.
    pub fn cleanup_temp_files(&self) {
        let count = self.temp_file_manager.file_count();
        if count > 0 {
            tracing::debug!("Cleaning up {} temp file(s)", count);
            self.temp_file_manager.cleanup();
        }
    }

    /// Dispatch GraphicEngineReady event to all registered handlers (async version)
    ///
    /// This is called when the graphic engine has been initialized and is ready
    /// to receive commands. This is a client-only event.
    pub async fn dispatch_graphic_engine_ready(
        &self,
        _request: &crate::api::GraphicEngineReadyRequest,
    ) -> crate::api::GraphicEngineReadyResponse {
        let handlers = self.system_api.event_dispatcher().get_handlers_for_graphic_engine_ready();

        if handlers.is_empty() {
            return crate::api::GraphicEngineReadyResponse::default();
        }

        debug!("Dispatching GraphicEngineReady to {} handlers", handlers.len());

        let mut response = crate::api::GraphicEngineReadyResponse::default();

        for handler in handlers {
            // Get the mod's context
            let loaded_mod = match self.loaded_mods.get(&handler.mod_id) {
                Some(m) => m,
                None => {
                    error!("Handler mod '{}' not loaded", handler.mod_id);
                    continue;
                }
            };

            let handler_id = handler.handler_id;
            let mod_id = handler.mod_id.clone();

            // Call the handler function with request and response objects
            let result: Result<bool, String> = loaded_mod
                .context
                .with(|ctx| {
                    // Get the handler function from the context's handler map
                    match bindings::get_js_handler(&ctx, handler_id) {
                        Ok(Some(func)) => {
                            // Create request object (empty for now, extensible in future)
                            let request_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create request object: {:?}", e))?;

                            // Create response object
                            let response_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create response object: {:?}", e))?;
                            response_obj.set("handled", false).map_err(|e| format!("Failed to set handled: {:?}", e))?;

                            // Add setHandled method
                            let set_handled = Function::new(ctx.clone(), |ctx: Ctx, handled: bool| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentGraphicEngineReadyResponse")?;
                                this.set("handled", handled)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setHandled: {:?}", e))?;
                            response_obj.set("setHandled", set_handled).map_err(|e| format!("Failed to set setHandled: {:?}", e))?;

                            // Store response object as global for method access
                            ctx.globals().set("__currentGraphicEngineReadyResponse", response_obj.clone()).map_err(|e| format!("Failed to set __currentGraphicEngineReadyResponse: {:?}", e))?;

                            // Call the handler function
                            let call_result = func.call::<(Object, Object), Value>((request_obj, response_obj.clone()));

                            match call_result {
                                Ok(result) => {
                                    // If result is a Promise, DO NOT call promise.finish() here!
                                    // The Promise will execute asynchronously via the JS event loop.
                                    // Calling finish() would block the tokio runtime and prevent
                                    // async operations (like createWindow's oneshot await) from completing.
                                    //
                                    // The Promise body will be executed by run_js_event_loop via
                                    // runtime.drive() which properly handles async operations.
                                    if result.is_promise() {
                                        //debug!("Handler in mod '{}' returned Promise - will execute asynchronously via event loop", mod_id);
                                        // Return true to indicate the handler was triggered successfully.
                                        // The actual async work will complete via run_js_event_loop.
                                        return Ok(true);
                                    }

                                    // Read back the response values (for synchronous handlers)
                                    let handled: bool = response_obj.get("handled").unwrap_or(false);
                                    Ok(handled)
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("Handler call error in mod '{}': {}", mod_id, error_msg);
                                    Err(format!("Handler call error: {}", error_msg))
                                }
                            }
                        }
                        Ok(None) => {
                            error!("Handler {} not found in mod '{}'", handler_id, mod_id);
                            Err(format!("Handler {} not found", handler_id))
                        }
                        Err(e) => {
                            error!("Failed to get handler {} from mod '{}': {:?}", handler_id, mod_id, e);
                            Err(format!("Failed to get handler: {:?}", e))
                        }
                    }
                })
                .await;

            // Update response based on handler result
            match result {
                Ok(handled) => {
                    response.handled = handled;

                    // If handler set handled=true, stop processing more handlers
                    // Note: For async handlers returning Promise, we set handled=true
                    // to indicate the handler was triggered (async completion via event loop)
                    if handled {
                        trace!("Handler in mod '{}' triggered for GraphicEngineReady", handler.mod_id);
                        break;
                    }
                }
                Err(e) => {
                    error!("Handler execution failed: {}", e);
                    // Continue to next handler on error
                }
            }
        }

        response
    }

    /// Dispatch GraphicEngineWindowClosed event to all registered handlers (async version)
    ///
    /// This is called when a window managed by the graphic engine is closed.
    /// The request contains the window_id of the closed window.
    /// This is a client-only event.
    pub async fn dispatch_graphic_engine_window_closed(
        &self,
        request: &crate::api::GraphicEngineWindowClosedRequest,
    ) -> crate::api::GraphicEngineWindowClosedResponse {
        let handlers = self.system_api.event_dispatcher().get_handlers_for_graphic_engine_window_closed();

        if handlers.is_empty() {
            return crate::api::GraphicEngineWindowClosedResponse::default();
        }

        debug!("Dispatching GraphicEngineWindowClosed (window_id={}) to {} handlers", request.window_id, handlers.len());

        let mut response = crate::api::GraphicEngineWindowClosedResponse::default();
        let window_id = request.window_id;

        for handler in handlers {
            // Get the mod's context
            let loaded_mod = match self.loaded_mods.get(&handler.mod_id) {
                Some(m) => m,
                None => {
                    error!("Handler mod '{}' not loaded", handler.mod_id);
                    continue;
                }
            };

            let handler_id = handler.handler_id;
            let mod_id = handler.mod_id.clone();

            // Call the handler function with request and response objects
            let result: Result<bool, String> = loaded_mod
                .context
                .with(|ctx| {
                    // Get the handler function from the context's handler map
                    match bindings::get_js_handler(&ctx, handler_id) {
                        Ok(Some(func)) => {
                            // Create request object with windowId
                            let request_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create request object: {:?}", e))?;
                            request_obj.set("windowId", window_id).map_err(|e| format!("Failed to set windowId: {:?}", e))?;

                            // Create response object
                            let response_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create response object: {:?}", e))?;
                            response_obj.set("handled", false).map_err(|e| format!("Failed to set handled: {:?}", e))?;

                            // Add setHandled method
                            let set_handled = Function::new(ctx.clone(), |ctx: Ctx, handled: bool| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentGraphicEngineWindowClosedResponse")?;
                                this.set("handled", handled)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setHandled: {:?}", e))?;
                            response_obj.set("setHandled", set_handled).map_err(|e| format!("Failed to set setHandled: {:?}", e))?;

                            // Store response object as global for method access
                            ctx.globals().set("__currentGraphicEngineWindowClosedResponse", response_obj.clone()).map_err(|e| format!("Failed to set __currentGraphicEngineWindowClosedResponse: {:?}", e))?;

                            // Call the handler function
                            let call_result = func.call::<(Object, Object), Value>((request_obj, response_obj.clone()));

                            match call_result {
                                Ok(result) => {
                                    // If result is a Promise, DO NOT call promise.finish() here!
                                    // The Promise will execute asynchronously via the JS event loop.
                                    if result.is_promise() {
                                        trace!("Handler in mod '{}' returned Promise - will execute asynchronously via event loop", mod_id);
                                        return Ok(true);
                                    }

                                    // Read back the response values (for synchronous handlers)
                                    let handled: bool = response_obj.get("handled").unwrap_or(false);
                                    Ok(handled)
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("Handler call error in mod '{}': {}", mod_id, error_msg);
                                    Err(format!("Handler call error: {}", error_msg))
                                }
                            }
                        }
                        Ok(None) => {
                            error!("Handler {} not found in mod '{}'", handler_id, mod_id);
                            Err(format!("Handler {} not found", handler_id))
                        }
                        Err(e) => {
                            error!("Failed to get handler {} from mod '{}': {:?}", handler_id, mod_id, e);
                            Err(format!("Failed to get handler: {:?}", e))
                        }
                    }
                })
                .await;

            // Update response based on handler result
            match result {
                Ok(handled) => {
                    response.handled = handled;

                    // If handler set handled=true, stop processing more handlers
                    if handled {
                        trace!("Handler in mod '{}' triggered for GraphicEngineWindowClosed", handler.mod_id);
                        break;
                    }
                }
                Err(e) => {
                    error!("Handler execution failed: {}", e);
                    // Continue to next handler on error
                }
            }
        }

        response
    }

    /// Dispatch a widget event to the registered callback
    ///
    /// This is called when a widget event occurs (click, hover, focus).
    /// It looks up the callback for the widget+event combination and invokes it.
    ///
    /// # Arguments
    /// * `widget_id` - The widget ID
    /// * `event_type` - Event type ("click", "hover", "focus")
    /// * `event_data` - Event-specific data as JSON object
    pub async fn dispatch_widget_event(
        &self,
        widget_id: u64,
        event_type: &str,
        event_data: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Dispatching widget event {} for widget {}", event_type, widget_id);

        // Iterate through all loaded mods to find which one has the callback
        for (mod_id, loaded_mod) in &self.loaded_mods {
            let event_type_owned = event_type.to_string();
            let event_data_str = serde_json::to_string(&event_data)?;

            let result: Result<bool, String> = loaded_mod
                .context
                .with(|ctx| {
                    // Try to get the widget callback from this context
                    match bindings::get_widget_handler(&ctx, widget_id, &event_type_owned) {
                        Ok(Some(func)) => {
                            // Found the callback! Parse event data and call it
                            let event_value: Value = ctx.json_parse(event_data_str.as_bytes())
                                .map_err(|e| format!("Failed to parse event data: {:?}", e))?;

                            let event_obj: Object = event_value.into_object()
                                .ok_or_else(|| "Event data is not an object".to_string())?;

                            // Call the callback with the event object
                            let call_result = func.call::<(Object,), Value>((event_obj,));

                            match call_result {
                                Ok(result) => {
                                    // If result is a Promise, it will execute asynchronously
                                    if result.is_promise() {
                                        debug!("Widget callback in mod '{}' returned Promise - will execute asynchronously", mod_id);
                                    }
                                    Ok(true)
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("Widget callback error in mod '{}': {}", mod_id, error_msg);
                                    Err(format!("Widget callback error: {}", error_msg))
                                }
                            }
                        }
                        Ok(None) => {
                            // No callback registered in this mod, continue to next
                            Ok(false)
                        }
                        Err(e) => {
                            error!("Failed to get widget handler from mod '{}': {:?}", mod_id, e);
                            Err(format!("Failed to get widget handler: {:?}", e))
                        }
                    }
                })
                .await;

            match result {
                Ok(true) => {
                    // Callback was found and executed
                    debug!("Widget event {} dispatched to mod '{}'", event_type, mod_id);
                    return Ok(());
                }
                Ok(false) => {
                    // No callback in this mod, continue
                    continue;
                }
                Err(e) => {
                    // Error occurred but we found the callback
                    return Err(e.into());
                }
            }
        }

        // No callback found in any mod
        debug!("No callback registered for widget {} event {}", widget_id, event_type);
        Ok(())
    }

    /// Dispatch a custom event to all registered handlers (async version)
    ///
    /// This is called when a mod calls `system.sendEvent(eventName, ...args)`.
    /// All handlers registered for the event are called in priority order (lowest first).
    /// Each handler receives a request object with `args` and a response object with
    /// `handled` and `results`.
    ///
    /// # Arguments
    /// * `request` - The custom event request containing event_name and args
    ///
    /// # Returns
    /// A `CustomEventResponse` containing whether the event was handled and any results
    pub async fn dispatch_custom_event(
        &self,
        request: &crate::api::CustomEventRequest,
    ) -> crate::api::CustomEventResponse {
        let handlers = self.system_api.event_dispatcher().get_handlers_for_custom_event(&request.event_name);

        if handlers.is_empty() {
            trace!("No handlers registered for custom event '{}'", request.event_name);
            return crate::api::CustomEventResponse::default();
        }

        trace!("Dispatching custom event '{}' to {} handlers", request.event_name, handlers.len());

        let mut response = crate::api::CustomEventResponse::default();
        let event_name = request.event_name.clone();
        let args = request.args.clone();

        for handler in handlers {
            // Get the mod's context
            let loaded_mod = match self.loaded_mods.get(&handler.mod_id) {
                Some(m) => m,
                None => {
                    error!("Handler mod '{}' not loaded", handler.mod_id);
                    continue;
                }
            };

            let handler_id = handler.handler_id;
            let mod_id = handler.mod_id.clone();
            let event_name_for_handler = event_name.clone();
            let args_for_handler = args.clone();

            // Step 1: Call the handler and detect if it returns a Promise
            let call_result: Result<bool, String> = loaded_mod
                .context
                .with(|ctx| {
                    // Get the handler function from the context's handler map
                    match bindings::get_js_handler(&ctx, handler_id) {
                        Ok(Some(func)) => {
                            // Create request object with event name and args
                            let request_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create request object: {:?}", e))?;
                            request_obj.set("eventName", event_name_for_handler.as_str()).map_err(|e| format!("Failed to set eventName: {:?}", e))?;

                            // Create args array from JSON strings
                            let js_args = rquickjs::Array::new(ctx.clone())
                                .map_err(|e| format!("Failed to create args array: {:?}", e))?;
                            for (i, arg) in args_for_handler.iter().enumerate() {
                                let js_value: Value = ctx.json_parse(arg.clone())
                                    .unwrap_or_else(|_| {
                                        rquickjs::String::from_str(ctx.clone(), arg)
                                            .map(|s| s.into())
                                            .unwrap_or(Value::new_undefined(ctx.clone()))
                                    });
                                js_args.set(i, js_value)
                                    .map_err(|e| format!("Failed to set arg {}: {:?}", i, e))?;
                            }
                            request_obj.set("args", js_args).map_err(|e| format!("Failed to set args: {:?}", e))?;

                            // Create response object with handled=false only
                            // Handlers can add any properties they want
                            let response_obj = Object::new(ctx.clone()).map_err(|e| format!("Failed to create response object: {:?}", e))?;
                            response_obj.set("handled", false).map_err(|e| format!("Failed to set handled: {:?}", e))?;

                            // Add setHandled method to response
                            let set_handled = Function::new(ctx.clone(), |ctx: Ctx, handled: bool| -> rquickjs::Result<()> {
                                let this: Object = ctx.globals().get("__currentCustomEventResponse")?;
                                this.set("handled", handled)?;
                                Ok(())
                            }).map_err(|e| format!("Failed to create setHandled: {:?}", e))?;
                            response_obj.set("setHandled", set_handled).map_err(|e| format!("Failed to set setHandled: {:?}", e))?;

                            // Store response object as global for method access and later retrieval
                            ctx.globals().set("__currentCustomEventResponse", response_obj.clone()).map_err(|e| format!("Failed to set __currentCustomEventResponse: {:?}", e))?;

                            // Call the handler function with request and response
                            let call_result = func.call::<(Object, Object), Value>((request_obj, response_obj));

                            match call_result {
                                Ok(result) => {
                                    // Return true if handler returned a Promise, false otherwise
                                    Ok(result.is_promise())
                                }
                                Err(e) => {
                                    let error_msg = Self::format_js_error(&ctx, &e);
                                    error!("Handler call error in mod '{}': {}", mod_id, error_msg);
                                    Err(format!("Handler call error: {}", error_msg))
                                }
                            }
                        }
                        Ok(None) => {
                            error!("Handler {} not found in mod '{}'", handler_id, mod_id);
                            Err(format!("Handler {} not found", handler_id))
                        }
                        Err(e) => {
                            error!("Failed to get handler {} from mod '{}': {:?}", handler_id, mod_id, e);
                            Err(format!("Failed to get handler: {:?}", e))
                        }
                    }
                })
                .await;

            // Check if handler call succeeded
            let was_promise = match call_result {
                Ok(is_promise) => is_promise,
                Err(e) => {
                    error!("Handler error: {}", e);
                    continue;
                }
            };

            // Step 2: We do NOT call runtime.idle() here, even with wait_for_completion=true.
            //
            // Reason: If sendEventAsync was called from JS, there's a pending job waiting for
            // the response. Calling idle() would wait for that job to complete, but that job
            // is waiting for us to send the response - causing a DEADLOCK.
            //
            // Instead, handlers MUST set response values (like res.handled = true) SYNCHRONOUSLY
            // before any await points. The async part of the handler will execute later via
            // the main JS event loop.
            if was_promise {
                trace!("Custom event handler returned Promise - reading sync values only (async work will complete via event loop)");
            }

            // Step 3: Read the response object
            // Note: For async handlers, the response values (like res.handled = true)
            // should be set synchronously before awaiting any async operation.
            let result: Result<(bool, std::collections::HashMap<String, String>), String> = loaded_mod
                .context
                .with(|ctx| {
                    // Get the response object from globals
                    let response_obj: Object = ctx.globals().get("__currentCustomEventResponse")
                        .map_err(|e| format!("Failed to get response object: {:?}", e))?;

                    // Read back the response values
                    let handled: bool = response_obj.get("handled").unwrap_or(false);

                    // Read all properties from the response object
                    let mut properties = std::collections::HashMap::new();
                    let keys = response_obj.keys::<String>();
                    for key_result in keys {
                        if let Ok(key) = key_result {
                            // Skip 'handled' and 'setHandled' - they are special
                            if key == "handled" || key == "setHandled" {
                                continue;
                            }
                            if let Ok(val) = response_obj.get::<_, Value>(&key) {
                                if let Ok(Some(json_str)) = ctx.json_stringify(val) {
                                    if let Ok(s) = json_str.to_string() {
                                        properties.insert(key, s);
                                    }
                                }
                            }
                        }
                    }

                    Ok((handled, properties))
                })
                .await;

            // Update response based on handler result
            match result {
                Ok((handled, properties)) => {
                    if handled {
                        response.handled = true;
                    }
                    // Merge properties from this handler into the response
                    for (key, value) in properties {
                        response.properties.insert(key, value);
                    }
                    trace!("Handler in mod '{}' executed for custom event '{}' (handled={})", handler.mod_id, event_name, handled);
                }
                Err(e) => {
                    error!("Handler execution failed: {}", e);
                    // Continue to next handler on error
                }
            }
        }

        response
    }
}

impl Drop for JsRuntimeAdapter {
    fn drop(&mut self) {
        // Cleanup temp files when the runtime is dropped
        self.cleanup_temp_files();
    }
}

// Synchronous trait implementation that wraps async calls
impl RuntimeAdapter for JsRuntimeAdapter {
    fn load_mod(
        &mut self,
        mod_path: &Path,
        mod_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.load_mod_async(mod_path, mod_id))
        })
    }

    fn call_mod_function(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.call_mod_function_async(mod_id, function_name))
        })
    }

    fn dispatch_widget_event(
        &self,
        widget_id: u64,
        event_type: &str,
        event_data: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.dispatch_widget_event(widget_id, event_type, event_data))
        })
    }

    fn call_mod_function_with_return(
        &mut self,
        mod_id: &str,
        function_name: &str,
    ) -> Result<ModReturnValue, Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.call_mod_function_with_return_async(mod_id, function_name))
        })
    }

    fn call_event_handler(
        &mut self,
        handler_id: u64,
        event_name: &str,
        args: &[String],
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.call_event_handler_async(handler_id, event_name, args))
        })
    }

    fn dispatch_terminal_key(
        &self,
        request: &crate::api::TerminalKeyRequest,
    ) -> crate::api::TerminalKeyResponse {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.dispatch_terminal_key(request))
        })
    }

    fn terminal_key_handler_count(&self) -> usize {
        self.system_api
            .event_dispatcher()
            .handler_count(crate::api::SystemEvents::TerminalKeyPressed)
    }

    fn dispatch_graphic_engine_ready(
        &self,
        request: &crate::api::GraphicEngineReadyRequest,
    ) -> crate::api::GraphicEngineReadyResponse {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.dispatch_graphic_engine_ready(request))
        })
    }

    fn dispatch_graphic_engine_window_closed(
        &self,
        request: &crate::api::GraphicEngineWindowClosedRequest,
    ) -> crate::api::GraphicEngineWindowClosedResponse {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.dispatch_graphic_engine_window_closed(request))
        })
    }

    fn dispatch_custom_event(
        &self,
        request: &crate::api::CustomEventRequest,
    ) -> crate::api::CustomEventResponse {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.dispatch_custom_event(request))
        })
    }
}

/// Run the JavaScript event loop
///
/// This function should be spawned as a task and run concurrently with other tasks.
/// It processes pending JavaScript jobs (Promises, timers spawned via ctx.spawn(), etc.)
///
/// The event loop will run until:
/// - Cancelled (e.g., via tokio::select with ctrl+c)
/// - A fatal JavaScript error occurs (unhandled promise rejection)
///
/// Returns `true` if a fatal error occurred, `false` otherwise.
///
/// Uses `runtime.drive()` which properly uses async Wakers to wait for new jobs
/// without busy-spinning. We use tokio::select! to also listen for fatal error signals.
pub async fn run_js_event_loop(runtime: Arc<AsyncRuntime>) -> bool {
    // Check for fatal error before starting
    if has_fatal_error() {
        error!("Fatal JavaScript error detected, terminating event loop");
        return true;
    }

    // Use tokio::select to wait for either:
    // 1. The JS runtime to complete (drive() never completes normally)
    // 2. A fatal error notification
    tokio::select! {
        biased;

        // Listen for fatal error signal
        _ = FATAL_ERROR_NOTIFY.notified() => {
            error!("Fatal JavaScript error detected, terminating event loop");
            true
        }

        // Run the JS event loop (this blocks until runtime is dropped)
        _ = runtime.drive() => {
            // drive() completed, check if it was due to a fatal error
            has_fatal_error()
        }
    }
}

/// Process all pending JavaScript jobs (Promises, etc.) and return whether a fatal error occurred.
///
/// This function should be called after operations that may spawn async JavaScript code
/// (like `onAttach` or `onBootstrap`) to ensure any unhandled Promise rejections are
/// detected immediately rather than later in the event loop.
///
/// Returns `true` if a fatal error was detected, `false` otherwise.
pub async fn flush_pending_jobs(runtime: &Arc<AsyncRuntime>) -> bool {
    // Process all pending jobs
    runtime.idle().await;

    // Check if any fatal error occurred during processing
    has_fatal_error()
}
