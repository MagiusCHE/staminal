//! Network API for Mod Runtimes
//!
//! Provides network operations for mods, primarily the `download` function
//! for fetching resources via the `stam://` protocol.
//!
//! # Architecture
//!
//! - **Client-side**: Connects to server via one-shot TCP connection
//! - **Server-side**: Not available (returns 501 Not Implemented)
//!
//! # Protocol Support
//!
//! - `stam://` - Staminal protocol (uses PrimalMessage::RequestUri)
//! - `http://` / `https://` - Returns 501 Not Implemented (future)

use std::sync::Arc;

/// Download response returned to JavaScript mods
#[derive(Debug, Clone)]
pub struct DownloadResponse {
    /// HTTP status code (200 = success, 404 = not found, 500 = error, etc.)
    pub status: u16,
    /// Response buffer data (if any)
    pub buffer: Option<Vec<u8>>,
    /// File name (if response is a file)
    pub file_name: Option<String>,
    /// File content (internal use only - will be saved to temp file before exposing to JS)
    pub file_content: Option<Vec<u8>>,
    /// Path to temp file containing the downloaded content (exposed to JS instead of file_content)
    pub temp_file_path: Option<String>,
}

impl Default for DownloadResponse {
    fn default() -> Self {
        Self {
            status: 0,
            buffer: None,
            file_name: None,
            file_content: None,
            temp_file_path: None,
        }
    }
}

/// Network API configuration
#[derive(Clone)]
pub struct NetworkConfig {
    /// Current game ID (required for stam:// requests)
    pub game_id: String,
    /// Default username for requests without credentials
    pub username: String,
    /// Default password hash for requests without credentials
    pub password_hash: String,
    /// Client version string
    pub client_version: String,
}

/// Callback type for performing the actual download operation
/// This is provided by the client/server to implement the actual network logic
pub type DownloadCallback = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = DownloadResponse> + Send>>
        + Send
        + Sync,
>;

/// Network API for mods
///
/// Provides network operations like downloading resources.
/// The actual network implementation is provided via a callback,
/// allowing client and server to have different implementations.
#[derive(Clone)]
pub struct NetworkApi {
    /// Configuration for network operations
    config: NetworkConfig,
    /// Callback to perform actual download
    download_callback: Option<DownloadCallback>,
}

impl NetworkApi {
    /// Create a new NetworkApi with the given configuration
    pub fn new(config: NetworkConfig) -> Self {
        Self {
            config,
            download_callback: None,
        }
    }

    /// Set the download callback
    pub fn set_download_callback(&mut self, callback: DownloadCallback) {
        self.download_callback = Some(callback);
    }

    /// Get the current game ID
    pub fn game_id(&self) -> &str {
        &self.config.game_id
    }

    /// Get the client version
    pub fn client_version(&self) -> &str {
        &self.config.client_version
    }

    /// Download a resource from the given URI
    ///
    /// # Arguments
    /// * `uri` - The URI to download from (stam://, http://, https://)
    ///
    /// # Returns
    /// A DownloadResponse with the result
    pub async fn download(&self, uri: &str) -> DownloadResponse {
        // Check protocol
        if uri.starts_with("stam://") {
            // Use the callback if available
            if let Some(callback) = &self.download_callback {
                return callback(uri.to_string()).await;
            }
            // No callback available
            DownloadResponse {
                status: 503,
                buffer: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            }
        } else if uri.starts_with("http://") || uri.starts_with("https://") {
            // HTTP(S) not implemented yet
            DownloadResponse {
                status: 501,
                buffer: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            }
        } else {
            // Unknown protocol
            DownloadResponse {
                status: 400,
                buffer: None,
                file_name: None,
                file_content: None,
                temp_file_path: None,
            }
        }
    }
}

/// Parse a stam:// URI and extract components
///
/// Format: stam://[username:password@]host:port/path[?query]
///
/// # Arguments
/// * `uri` - The URI to parse
///
/// # Returns
/// Tuple of (host_port, path, username, password)
pub fn parse_stam_uri(uri: &str) -> Option<(String, String, Option<String>, Option<String>)> {
    let without_scheme = uri.strip_prefix("stam://")?;

    // Check for credentials
    let (credentials, rest) = if let Some(at_pos) = without_scheme.find('@') {
        let creds = &without_scheme[..at_pos];
        let rest = &without_scheme[at_pos + 1..];

        // Parse username:password
        let (username, password) = if let Some(colon_pos) = creds.find(':') {
            (
                Some(creds[..colon_pos].to_string()),
                Some(creds[colon_pos + 1..].to_string()),
            )
        } else {
            (Some(creds.to_string()), None)
        };

        (Some((username, password)), rest)
    } else {
        (None, without_scheme)
    };

    // Split host:port from path
    let (host_port, path) = if let Some(slash_pos) = rest.find('/') {
        (rest[..slash_pos].to_string(), rest[slash_pos..].to_string())
    } else {
        (rest.to_string(), "/".to_string())
    };

    let (username, password) = credentials.unwrap_or((None, None));

    Some((host_port, path, username, password))
}

/// Sanitize a URI by removing credentials
///
/// # Arguments
/// * `uri` - The URI to sanitize
///
/// # Returns
/// The URI without credentials
pub fn sanitize_uri(uri: &str) -> String {
    if let Some((host_port, path, _, _)) = parse_stam_uri(uri) {
        format!("stam://{}{}", host_port, path)
    } else {
        uri.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stam_uri_simple() {
        let result = parse_stam_uri("stam://localhost:9999/mods-manager/download");
        assert!(result.is_some());
        let (host_port, path, username, password) = result.unwrap();
        assert_eq!(host_port, "localhost:9999");
        assert_eq!(path, "/mods-manager/download");
        assert!(username.is_none());
        assert!(password.is_none());
    }

    #[test]
    fn test_parse_stam_uri_with_credentials() {
        let result = parse_stam_uri("stam://user:pass@localhost:9999/path");
        assert!(result.is_some());
        let (host_port, path, username, password) = result.unwrap();
        assert_eq!(host_port, "localhost:9999");
        assert_eq!(path, "/path");
        assert_eq!(username, Some("user".to_string()));
        assert_eq!(password, Some("pass".to_string()));
    }

    #[test]
    fn test_parse_stam_uri_no_path() {
        let result = parse_stam_uri("stam://localhost:9999");
        assert!(result.is_some());
        let (host_port, path, _, _) = result.unwrap();
        assert_eq!(host_port, "localhost:9999");
        assert_eq!(path, "/");
    }

    #[test]
    fn test_sanitize_uri() {
        let sanitized = sanitize_uri("stam://user:pass@localhost:9999/path");
        assert_eq!(sanitized, "stam://localhost:9999/path");
    }

    #[test]
    fn test_sanitize_uri_no_credentials() {
        let sanitized = sanitize_uri("stam://localhost:9999/path");
        assert_eq!(sanitized, "stam://localhost:9999/path");
    }
}
