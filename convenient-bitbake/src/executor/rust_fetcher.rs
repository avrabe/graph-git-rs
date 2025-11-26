//! Pure Rust source fetcher - no host tools required
//!
//! This module implements source fetching using only pure Rust libraries:
//! - **HTTP/HTTPS**: `ureq` with rustls for TLS (no wget/curl)
//! - **Git**: `git2` (libgit2 bindings) with proxy support (no git CLI)
//! - **Checksums**: `sha2`, `md5` for verification
//! - **Archives**: `tar`, `flate2`, `bzip2`, `xz2` for extraction
//!
//! ## Proxy Support
//!
//! Proxies are automatically detected from environment variables:
//! - `HTTP_PROXY` / `http_proxy`
//! - `HTTPS_PROXY` / `https_proxy`
//! - `ALL_PROXY` / `all_proxy`
//! - `NO_PROXY` / `no_proxy`
//!
//! ## TLS Support
//!
//! Uses rustls (pure Rust TLS) by default. No OpenSSL dependency.

use crate::{SourceUri, UriScheme};
use git2::{
    build::CheckoutBuilder, Cred, FetchOptions, ProxyOptions, RemoteCallbacks, Repository,
};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during source fetching
#[derive(Debug, Error)]
pub enum FetchError {
    #[error("Unsupported URI scheme: {0:?}")]
    UnsupportedScheme(UriScheme),

    #[error("Git operation failed: {0}")]
    GitError(#[from] git2::Error),

    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("File operation failed: {0}")]
    FileError(String),

    #[error("Checksum mismatch for {file}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        file: String,
        expected: String,
        actual: String,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Download incomplete: expected {expected} bytes, got {actual}")]
    IncompleteDownload { expected: u64, actual: u64 },
}

pub type FetchResult<T> = Result<T, FetchError>;

/// Progress callback for long-running operations
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send>;

/// Fetch configuration
#[derive(Default)]
pub struct FetchConfig {
    /// Git username for authenticated repos
    pub git_user: Option<String>,
    /// Git password/token for authenticated repos
    pub git_password: Option<String>,
    /// Custom proxy URL (overrides environment)
    pub proxy_url: Option<String>,
    /// Progress callback
    pub progress: Option<ProgressCallback>,
    /// Allow insecure TLS (not recommended)
    pub insecure: bool,
}

/// Download source from SRC_URI to downloads directory (pure Rust)
///
/// # Arguments
///
/// * `src_uri` - Parsed SRC_URI from recipe
/// * `downloads_dir` - Directory to store downloaded files (DL_DIR)
/// * `config` - Optional fetch configuration
///
/// # Returns
///
/// Path to the downloaded file or cloned repository
pub fn fetch_source(
    src_uri: &SourceUri,
    downloads_dir: &Path,
    config: Option<&FetchConfig>,
) -> FetchResult<PathBuf> {
    info!("Fetching source: {}", src_uri.url);
    debug!("  Scheme: {:?}", src_uri.scheme);
    debug!("  Branch: {:?}", src_uri.branch);
    debug!("  Tag: {:?}", src_uri.tag);
    debug!("  SRCREV: {:?}", src_uri.srcrev);

    fs::create_dir_all(downloads_dir)?;

    let default_config = FetchConfig::default();
    let config = config.unwrap_or(&default_config);

    match src_uri.scheme {
        UriScheme::Git | UriScheme::GitSubmodule => fetch_git(src_uri, downloads_dir, config),
        UriScheme::Http | UriScheme::Https => fetch_http(src_uri, downloads_dir, config),
        UriScheme::File => fetch_file(src_uri, downloads_dir),
        _ => Err(FetchError::UnsupportedScheme(src_uri.scheme.clone())),
    }
}

/// Verify checksum of downloaded file
pub fn verify_checksum(file_path: &Path, expected_sha256: &str) -> FetchResult<()> {
    info!("Verifying checksum: {}", file_path.display());

    let mut file = File::open(file_path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let actual = hex::encode(hasher.finalize());

    if actual.eq_ignore_ascii_case(expected_sha256) {
        info!("Checksum verified: {}", &actual[..16]);
        Ok(())
    } else {
        Err(FetchError::ChecksumMismatch {
            file: file_path.display().to_string(),
            expected: expected_sha256.to_string(),
            actual,
        })
    }
}

/// Verify MD5 checksum (for legacy SRC_URI[md5sum])
pub fn verify_md5(file_path: &Path, expected_md5: &str) -> FetchResult<()> {
    use md5::{Digest as Md5Digest, Md5};

    info!("Verifying MD5: {}", file_path.display());

    let mut file = File::open(file_path)?;
    let mut hasher = Md5::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let actual = hex::encode(hasher.finalize());

    if actual.eq_ignore_ascii_case(expected_md5) {
        info!("MD5 verified: {}", actual);
        Ok(())
    } else {
        Err(FetchError::ChecksumMismatch {
            file: file_path.display().to_string(),
            expected: expected_md5.to_string(),
            actual,
        })
    }
}

// ============================================================================
// Git Fetcher (using git2 - pure Rust bindings to libgit2)
// ============================================================================

fn fetch_git(src_uri: &SourceUri, downloads_dir: &Path, config: &FetchConfig) -> FetchResult<PathBuf> {
    let repo_name = extract_repo_name(&src_uri.url)?;
    let dest_dir = downloads_dir.join("git").join(&repo_name);

    // Create parent directory
    if let Some(parent) = dest_dir.parent() {
        fs::create_dir_all(parent)?;
    }

    if dest_dir.join(".git").exists() || dest_dir.join("HEAD").exists() {
        info!("Repository exists, updating: {}", dest_dir.display());
        update_git_repo(&dest_dir, src_uri, config)
    } else {
        info!("Cloning repository: {} -> {}", src_uri.url, dest_dir.display());
        clone_git_repo(src_uri, &dest_dir, config)?;
        Ok(dest_dir)
    }
}

fn clone_git_repo(src_uri: &SourceUri, dest_dir: &Path, config: &FetchConfig) -> FetchResult<()> {
    // Progress tracking
    let progress_state = RefCell::new((0usize, 0usize));

    // Setup callbacks
    let mut callbacks = RemoteCallbacks::new();

    // Credential handling
    let git_user = config.git_user.clone();
    let git_password = config.git_password.clone();

    callbacks.credentials(move |_url, username_from_url, allowed_types| {
        // Try SSH agent first
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(username) = username_from_url {
                if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                    return Ok(cred);
                }
            }
        }

        // Try username/password
        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            if let (Some(ref user), Some(ref pass)) = (&git_user, &git_password) {
                return Cred::userpass_plaintext(user, pass);
            }
        }

        // Try default credentials
        if allowed_types.contains(git2::CredentialType::DEFAULT) {
            return Cred::default();
        }

        Err(git2::Error::from_str("no credentials available"))
    });

    // Progress callback
    callbacks.transfer_progress(|stats| {
        let mut state = progress_state.borrow_mut();
        let received = stats.received_objects();
        let total = stats.total_objects();

        if received != state.0 || total != state.1 {
            state.0 = received;
            state.1 = total;

            if total > 0 {
                let pct = (received * 100) / total;
                if pct % 10 == 0 || received == total {
                    info!(
                        "Git progress: {}/{} objects ({}%)",
                        received, total, pct
                    );
                }
            }
        }
        true
    });

    // Setup fetch options with proxy
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    // Proxy configuration
    let mut proxy_opts = ProxyOptions::new();
    if let Some(ref proxy_url) = config.proxy_url {
        proxy_opts.url(proxy_url);
    } else {
        proxy_opts.auto(); // Use HTTP_PROXY, HTTPS_PROXY environment variables
    }
    fetch_opts.proxy_options(proxy_opts);

    // Checkout options
    let mut checkout_opts = CheckoutBuilder::new();
    checkout_opts.force();

    // Build and clone
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);
    builder.with_checkout(checkout_opts);

    // Clone specific branch if specified
    if let Some(ref branch) = src_uri.branch {
        if !src_uri.nobranch {
            builder.branch(branch);
        }
    }

    // Perform clone
    let repo = builder.clone(&src_uri.url, dest_dir)?;
    info!("Clone completed: {}", dest_dir.display());

    // Checkout specific revision if needed
    if let Some(ref srcrev) = src_uri.srcrev {
        checkout_revision(&repo, srcrev)?;
    } else if let Some(ref tag) = src_uri.tag {
        checkout_revision(&repo, tag)?;
    }

    Ok(())
}

fn update_git_repo(
    repo_dir: &Path,
    src_uri: &SourceUri,
    config: &FetchConfig,
) -> FetchResult<PathBuf> {
    let repo = Repository::open(repo_dir)?;

    // Setup callbacks for fetch
    let mut callbacks = RemoteCallbacks::new();
    let git_user = config.git_user.clone();
    let git_password = config.git_password.clone();

    callbacks.credentials(move |_url, username_from_url, allowed_types| {
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(username) = username_from_url {
                if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                    return Ok(cred);
                }
            }
        }
        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            if let (Some(ref user), Some(ref pass)) = (&git_user, &git_password) {
                return Cred::userpass_plaintext(user, pass);
            }
        }
        Cred::default()
    });

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut proxy_opts = ProxyOptions::new();
    if let Some(ref proxy_url) = config.proxy_url {
        proxy_opts.url(proxy_url);
    } else {
        proxy_opts.auto();
    }
    fetch_opts.proxy_options(proxy_opts);

    // Fetch from origin
    let mut remote = repo.find_remote("origin")?;
    let refspecs: &[&str] = &[];
    remote.fetch(refspecs, Some(&mut fetch_opts), None)?;
    info!("Fetch completed");

    // Checkout requested revision
    if let Some(ref srcrev) = src_uri.srcrev {
        checkout_revision(&repo, srcrev)?;
    } else if let Some(ref tag) = src_uri.tag {
        checkout_revision(&repo, tag)?;
    } else if let Some(ref branch) = src_uri.branch {
        checkout_revision(&repo, &format!("origin/{}", branch))?;
    }

    Ok(repo_dir.to_path_buf())
}

fn checkout_revision(repo: &Repository, revision: &str) -> FetchResult<()> {
    info!("Checking out: {}", revision);

    // Try to resolve the revision
    let obj = repo.revparse_single(revision)?;

    // Checkout
    let mut checkout_opts = CheckoutBuilder::new();
    checkout_opts.force();

    repo.checkout_tree(&obj, Some(&mut checkout_opts))?;

    // Set HEAD
    if let Ok(commit) = obj.peel_to_commit() {
        repo.set_head_detached(commit.id())?;
    }

    info!("Checked out: {}", revision);
    Ok(())
}

// ============================================================================
// HTTP/HTTPS Fetcher (using ureq - pure Rust)
// ============================================================================

fn fetch_http(
    src_uri: &SourceUri,
    downloads_dir: &Path,
    config: &FetchConfig,
) -> FetchResult<PathBuf> {
    let filename = extract_filename(&src_uri.url)?;
    let dest_file = downloads_dir.join(&filename);

    // Skip if already downloaded
    if dest_file.exists() {
        info!("File already exists: {}", dest_file.display());
        return Ok(dest_file);
    }

    info!("Downloading: {} -> {}", src_uri.url, dest_file.display());

    // Build ureq agent with proxy support
    let mut agent_builder = ureq::AgentBuilder::new();

    // Set proxy if configured or from environment
    if let Some(ref proxy_url) = config.proxy_url {
        if let Ok(proxy) = ureq::Proxy::new(proxy_url) {
            agent_builder = agent_builder.proxy(proxy);
        }
    } else {
        // Auto-detect from environment
        if let Some(proxy_url) = get_proxy_for_url(&src_uri.url) {
            if let Ok(proxy) = ureq::Proxy::new(&proxy_url) {
                agent_builder = agent_builder.proxy(proxy);
                debug!("Using proxy: {}", proxy_url);
            }
        }
    }

    let agent = agent_builder.build();

    // Make request
    let response = agent.get(&src_uri.url).call().map_err(|e| {
        FetchError::HttpError(format!("Request failed: {}", e))
    })?;

    // Get content length if available
    let content_length: Option<u64> = response
        .header("content-length")
        .and_then(|s| s.parse().ok());

    if let Some(len) = content_length {
        info!("Downloading {} bytes", len);
    }

    // Create temporary file for download
    let temp_file = dest_file.with_extension("tmp");
    let mut file = File::create(&temp_file)?;

    // Download with progress
    let mut reader = response.into_reader();
    let mut buffer = [0u8; 65536]; // 64KB buffer
    let mut total_bytes: u64 = 0;
    let mut last_progress = 0u64;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])?;
        total_bytes += bytes_read as u64;

        // Progress reporting every 1MB
        if total_bytes - last_progress >= 1_048_576 {
            if let Some(len) = content_length {
                let pct = (total_bytes * 100) / len;
                info!("Download progress: {} / {} bytes ({}%)", total_bytes, len, pct);
            } else {
                info!("Download progress: {} bytes", total_bytes);
            }
            last_progress = total_bytes;
        }
    }

    file.flush()?;
    drop(file);

    // Verify download size if known
    if let Some(expected) = content_length {
        if total_bytes != expected {
            let _ = fs::remove_file(&temp_file);
            return Err(FetchError::IncompleteDownload {
                expected,
                actual: total_bytes,
            });
        }
    }

    // Move temp file to final location
    fs::rename(&temp_file, &dest_file)?;

    info!("Downloaded: {} ({} bytes)", dest_file.display(), total_bytes);
    Ok(dest_file)
}

/// Get proxy URL from environment for a given URL
fn get_proxy_for_url(url: &str) -> Option<String> {
    // Check NO_PROXY first
    let no_proxy = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();

    if !no_proxy.is_empty() {
        // Extract host from URL
        if let Some(host) = extract_host(url) {
            for pattern in no_proxy.split(',') {
                let pattern = pattern.trim();
                if pattern == "*" || host.ends_with(pattern) || host == pattern {
                    return None;
                }
            }
        }
    }

    // Check protocol-specific proxy
    if url.starts_with("https://") {
        std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .ok()
    } else if url.starts_with("http://") {
        std::env::var("HTTP_PROXY")
            .or_else(|_| std::env::var("http_proxy"))
            .ok()
    } else {
        None
    }
    .or_else(|| {
        std::env::var("ALL_PROXY")
            .or_else(|_| std::env::var("all_proxy"))
            .ok()
    })
}

fn extract_host(url: &str) -> Option<String> {
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = url.split('/').next()?;
    let host = host.split(':').next()?; // Remove port
    Some(host.to_string())
}

// ============================================================================
// File Fetcher (local copy)
// ============================================================================

fn fetch_file(src_uri: &SourceUri, downloads_dir: &Path) -> FetchResult<PathBuf> {
    let src_path = src_uri
        .url
        .strip_prefix("file://")
        .unwrap_or(&src_uri.url);

    let src = Path::new(src_path);
    let filename = src
        .file_name()
        .ok_or_else(|| FetchError::InvalidUrl(format!("No filename in: {}", src_uri.url)))?;

    let dest = downloads_dir.join(filename);

    if dest.exists() {
        info!("File already exists: {}", dest.display());
        return Ok(dest);
    }

    info!("Copying: {} -> {}", src.display(), dest.display());

    fs::copy(src, &dest).map_err(|e| FetchError::FileError(e.to_string()))?;

    info!("Copied: {}", dest.display());
    Ok(dest)
}

// ============================================================================
// Utilities
// ============================================================================

fn extract_repo_name(url: &str) -> FetchResult<String> {
    let url_clean = url.trim_end_matches('/');

    let name = url_clean
        .rsplit('/')
        .next()
        .ok_or_else(|| FetchError::InvalidUrl(format!("Cannot extract repo name from: {}", url)))?
        .trim_end_matches(".git");

    if name.is_empty() {
        return Err(FetchError::InvalidUrl(format!(
            "Empty repo name in: {}",
            url
        )));
    }

    Ok(name.to_string())
}

fn extract_filename(url: &str) -> FetchResult<String> {
    let url_clean = url.trim_end_matches('/');

    let name = url_clean
        .rsplit('/')
        .next()
        .ok_or_else(|| FetchError::InvalidUrl(format!("Cannot extract filename from: {}", url)))?;

    // Remove query parameters
    let name_clean = name.split('?').next().unwrap_or(name);

    if name_clean.is_empty() {
        return Err(FetchError::InvalidUrl(format!("Empty filename in: {}", url)));
    }

    Ok(name_clean.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_repo_name() {
        assert_eq!(
            extract_repo_name("https://github.com/foo/bar.git").unwrap(),
            "bar"
        );
        assert_eq!(
            extract_repo_name("git://git.yoctoproject.org/poky").unwrap(),
            "poky"
        );
    }

    #[test]
    fn test_extract_filename() {
        assert_eq!(
            extract_filename("https://example.com/file.tar.gz").unwrap(),
            "file.tar.gz"
        );
        assert_eq!(
            extract_filename("http://example.com/path/archive.zip?v=1").unwrap(),
            "archive.zip"
        );
    }

    #[test]
    fn test_get_proxy_for_url() {
        // This test depends on environment, so just verify it doesn't panic
        let _ = get_proxy_for_url("https://example.com/file.tar.gz");
    }

    #[test]
    fn test_extract_host() {
        assert_eq!(
            extract_host("https://example.com/path"),
            Some("example.com".to_string())
        );
        assert_eq!(
            extract_host("http://example.com:8080/path"),
            Some("example.com".to_string())
        );
    }
}
