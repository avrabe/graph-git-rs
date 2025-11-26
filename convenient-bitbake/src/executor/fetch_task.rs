//! Fetch task implementation - handles do_fetch using pure Rust
//!
//! This module provides the integration layer between the task executor
//! and the pure Rust fetcher. It:
//! - Parses SRC_URI from recipe variables
//! - Downloads sources using rust_fetcher
//! - Verifies checksums
//! - Places files in DL_DIR

use super::rust_fetcher::{self, FetchConfig, FetchError, FetchResult};
use crate::{SourceUri, UriScheme};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Result of a fetch task execution
#[derive(Debug)]
pub struct FetchTaskResult {
    /// Downloaded file paths
    pub downloaded_files: Vec<PathBuf>,
    /// Any warnings encountered
    pub warnings: Vec<String>,
    /// Total bytes downloaded
    pub total_bytes: u64,
}

/// Execute a fetch task for a recipe
///
/// This is the main entry point for do_fetch task execution.
/// It parses SRC_URI from the recipe variables and downloads all sources.
///
/// # Arguments
///
/// * `recipe_vars` - Recipe variables including SRC_URI, SRC_URI[md5sum], etc.
/// * `dl_dir` - Download directory (DL_DIR)
/// * `config` - Optional fetch configuration
///
/// # Returns
///
/// FetchTaskResult with downloaded files, or error
pub fn execute_fetch_task(
    recipe_vars: &HashMap<String, String>,
    dl_dir: &Path,
    config: Option<&FetchConfig>,
) -> FetchResult<FetchTaskResult> {
    info!("Executing fetch task");
    debug!("DL_DIR: {}", dl_dir.display());

    // Parse SRC_URI
    let src_uri_str = recipe_vars
        .get("SRC_URI")
        .ok_or_else(|| FetchError::InvalidUrl("No SRC_URI defined".to_string()))?;

    info!("SRC_URI: {}", src_uri_str);

    // Parse all URIs from SRC_URI (space-separated, may have line continuations)
    let uris = parse_src_uri_list(src_uri_str, recipe_vars)?;
    info!("Found {} source URIs", uris.len());

    let mut downloaded_files = Vec::new();
    let mut warnings = Vec::new();
    let mut total_bytes = 0u64;

    for uri in &uris {
        info!("Fetching: {}", uri.url);

        match rust_fetcher::fetch_source(uri, dl_dir, config) {
            Ok(path) => {
                // Verify checksum if provided
                let uri_key = extract_uri_key(&uri.url);

                // Check for SHA256
                let sha256_key = format!("SRC_URI[{}][sha256sum]", uri_key);
                let sha256_alt = format!("SRC_URI[sha256sum]");
                if let Some(expected) = recipe_vars.get(&sha256_key).or_else(|| recipe_vars.get(&sha256_alt)) {
                    if !expected.is_empty() {
                        rust_fetcher::verify_checksum(&path, expected)?;
                    }
                }

                // Check for MD5
                let md5_key = format!("SRC_URI[{}][md5sum]", uri_key);
                let md5_alt = format!("SRC_URI[md5sum]");
                if let Some(expected) = recipe_vars.get(&md5_key).or_else(|| recipe_vars.get(&md5_alt)) {
                    if !expected.is_empty() {
                        rust_fetcher::verify_md5(&path, expected)?;
                    }
                }

                // Get file size
                if let Ok(metadata) = std::fs::metadata(&path) {
                    total_bytes += metadata.len();
                }

                downloaded_files.push(path);
            }
            Err(e) => {
                // For file:// URIs, we might not find local patches - just warn
                if matches!(uri.scheme, UriScheme::File) {
                    warnings.push(format!("Local file not found: {} ({})", uri.url, e));
                } else {
                    return Err(e);
                }
            }
        }
    }

    info!(
        "Fetch complete: {} files, {} bytes",
        downloaded_files.len(),
        total_bytes
    );

    Ok(FetchTaskResult {
        downloaded_files,
        warnings,
        total_bytes,
    })
}

/// Parse SRC_URI string into list of SourceUri
///
/// Handles:
/// - Space-separated URLs
/// - Line continuations with backslash
/// - BitBake parameters (;param=value)
/// - Variable expansion (${PV}, ${PN}, etc.)
fn parse_src_uri_list(
    src_uri_str: &str,
    vars: &HashMap<String, String>,
) -> FetchResult<Vec<SourceUri>> {
    let mut uris = Vec::new();

    // Normalize line continuations
    let normalized = src_uri_str
        .replace("\\\n", " ")
        .replace("\\", "");

    // Split by whitespace
    for part in normalized.split_whitespace() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Expand variables
        let expanded = expand_variables(part, vars);

        // Parse URI with parameters
        let uri = parse_single_uri(&expanded)?;
        uris.push(uri);
    }

    Ok(uris)
}

/// Parse a single URI with BitBake parameters
///
/// Format: `scheme://host/path;param1=value1;param2=value2`
fn parse_single_uri(uri_str: &str) -> FetchResult<SourceUri> {
    // Split URL from parameters
    let mut parts = uri_str.splitn(2, ';');
    let url = parts.next().unwrap_or(uri_str);
    let params_str = parts.next().unwrap_or("");

    // Parse parameters
    let mut params: HashMap<String, String> = HashMap::new();
    for param in params_str.split(';') {
        if let Some((key, value)) = param.split_once('=') {
            params.insert(key.to_string(), value.to_string());
        } else if !param.is_empty() {
            // Boolean parameter like "nobranch"
            params.insert(param.to_string(), "1".to_string());
        }
    }

    // Determine scheme
    let scheme = if url.starts_with("git://") || url.starts_with("git@") {
        UriScheme::Git
    } else if url.starts_with("https://") {
        // Check if it's a git repo
        if url.contains(".git") || params.contains_key("protocol") {
            UriScheme::Git
        } else {
            UriScheme::Https
        }
    } else if url.starts_with("http://") {
        UriScheme::Http
    } else if url.starts_with("file://") || !url.contains("://") {
        UriScheme::File
    } else if url.starts_with("ftp://") {
        UriScheme::Other("ftp".to_string())
    } else if url.starts_with("svn://") {
        UriScheme::Other("svn".to_string())
    } else {
        // Default to HTTPS for unknown schemes
        UriScheme::Https
    };

    // Convert git:// to https:// if protocol=https is specified
    let url = if scheme == UriScheme::Git && params.get("protocol").map(|s| s.as_str()) == Some("https") {
        url.replacen("git://", "https://", 1)
    } else {
        url.to_string()
    };

    Ok(SourceUri {
        raw: uri_str.to_string(),
        scheme,
        url,
        protocol: params.get("protocol").cloned(),
        branch: params.get("branch").cloned(),
        tag: params.get("tag").cloned(),
        srcrev: params.get("rev").or_else(|| params.get("srcrev")).cloned(),
        destsuffix: params.get("destsuffix").cloned(),
        nobranch: params.contains_key("nobranch"),
    })
}

/// Expand BitBake variables in a string
fn expand_variables(s: &str, vars: &HashMap<String, String>) -> String {
    let mut result = s.to_string();

    // Simple variable expansion: ${VAR}
    for (key, value) in vars {
        let pattern = format!("${{{}}}", key);
        result = result.replace(&pattern, value);
    }

    // Also handle $VAR format (less common but used)
    for (key, value) in vars {
        let pattern = format!("${}", key);
        // Only replace if not followed by { (to avoid double expansion)
        if !result.contains(&format!("${{{}}}", key)) {
            result = result.replace(&pattern, value);
        }
    }

    result
}

/// Extract a key from URL for SRC_URI[key][param] lookups
fn extract_uri_key(url: &str) -> String {
    // Use the filename or repo name as key
    url.rsplit('/')
        .next()
        .unwrap_or(url)
        .trim_end_matches(".git")
        .split('?')
        .next()
        .unwrap_or(url)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_uri_http() {
        let uri = parse_single_uri("https://example.com/file.tar.gz").unwrap();
        assert_eq!(uri.scheme, UriScheme::Https);
        assert_eq!(uri.url, "https://example.com/file.tar.gz");
    }

    #[test]
    fn test_parse_single_uri_git_with_params() {
        let uri = parse_single_uri("git://github.com/foo/bar.git;branch=main;protocol=https").unwrap();
        assert_eq!(uri.scheme, UriScheme::Git);
        assert!(uri.url.contains("https://"));
        assert_eq!(uri.branch, Some("main".to_string()));
    }

    #[test]
    fn test_parse_single_uri_file() {
        let uri = parse_single_uri("file://defconfig").unwrap();
        assert_eq!(uri.scheme, UriScheme::File);
    }

    #[test]
    fn test_expand_variables() {
        let mut vars = HashMap::new();
        vars.insert("PV".to_string(), "1.36.1".to_string());
        vars.insert("PN".to_string(), "busybox".to_string());

        let result = expand_variables("https://example.com/${PN}-${PV}.tar.gz", &vars);
        assert_eq!(result, "https://example.com/busybox-1.36.1.tar.gz");
    }

    #[test]
    fn test_parse_src_uri_list() {
        let vars = HashMap::new();
        let uri_str = "https://example.com/a.tar.gz \
                       https://example.com/b.tar.gz";
        let uris = parse_src_uri_list(uri_str, &vars).unwrap();
        assert_eq!(uris.len(), 2);
    }
}
