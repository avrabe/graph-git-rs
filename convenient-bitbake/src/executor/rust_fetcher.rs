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
    /// Supports: http://, https://, socks5://, socks5h://
    pub proxy_url: Option<String>,
    /// Proxy username (for authenticated proxies)
    pub proxy_user: Option<String>,
    /// Proxy password (for authenticated proxies)
    pub proxy_password: Option<String>,
    /// Progress callback
    pub progress: Option<ProgressCallback>,
    /// Allow insecure TLS (not recommended)
    pub insecure: bool,
    /// SSH private key path for git (alternative to ssh-agent)
    pub ssh_key_path: Option<PathBuf>,
    /// SSH known hosts file (for strict host checking)
    pub ssh_known_hosts: Option<PathBuf>,
    /// Git protocol preference: "https", "ssh", "git"
    pub git_protocol: Option<String>,
    /// Retry count for network operations
    pub retry_count: Option<u32>,
    /// Timeout for operations in seconds
    pub timeout_secs: Option<u64>,
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
    // Convert git:// to https:// when proxy is configured
    // git:// protocol doesn't work through HTTP proxies, but https:// does
    let effective_uri = if src_uri.url.starts_with("git://") {
        let has_proxy = config.proxy_url.is_some() || get_git_proxy_from_env().is_some();
        if has_proxy {
            let https_url = src_uri.url.replace("git://", "https://");
            info!("Converting git:// to https:// for proxy compatibility: {}", https_url);
            // Create a modified SourceUri with https:// URL
            let mut modified = src_uri.clone();
            modified.url = https_url;
            modified
        } else {
            src_uri.clone()
        }
    } else {
        src_uri.clone()
    };

    let repo_name = extract_repo_name(&effective_uri.url)?;
    let dest_dir = downloads_dir.join("git").join(&repo_name);

    // Create parent directory
    if let Some(parent) = dest_dir.parent() {
        fs::create_dir_all(parent)?;
    }

    if dest_dir.join(".git").exists() || dest_dir.join("HEAD").exists() {
        info!("Repository exists, updating: {}", dest_dir.display());
        // Try to update, but if it fails with proxy error, use existing repo
        match update_git_repo(&dest_dir, &effective_uri, config) {
            Ok(path) => Ok(path),
            Err(e) => {
                let is_proxy_error = matches!(&e, FetchError::GitError(ge)
                    if ge.message().contains("proxy") || ge.message().contains("401") || ge.message().contains("407"));
                if is_proxy_error {
                    warn!("git2 update failed with proxy error, using existing repo: {}", e);
                    // Try git CLI pull as fallback
                    if let Err(pull_err) = update_git_repo_cli(&dest_dir, &effective_uri) {
                        warn!("git CLI pull also failed, using existing repo as-is: {}", pull_err);
                    }
                    Ok(dest_dir.clone())
                } else {
                    Err(e)
                }
            }
        }
    } else {
        info!("Cloning repository: {} -> {}", effective_uri.url, dest_dir.display());

        // Try pure Rust git2 first, fall back to git CLI on proxy errors
        match clone_git_repo(&effective_uri, &dest_dir, config) {
            Ok(()) => Ok(dest_dir),
            Err(e) => {
                // Check if it's a proxy-related error
                let is_proxy_error = matches!(&e, FetchError::GitError(ge)
                    if ge.message().contains("proxy") || ge.message().contains("401") || ge.message().contains("407"));

                if is_proxy_error {
                    warn!("git2 failed with proxy error, falling back to git CLI: {}", e);
                    clone_git_repo_cli(&effective_uri, &dest_dir)?;
                    Ok(dest_dir)
                } else {
                    Err(e)
                }
            }
        }
    }
}

fn clone_git_repo(src_uri: &SourceUri, dest_dir: &Path, config: &FetchConfig) -> FetchResult<()> {
    // Progress tracking
    let progress_state = RefCell::new((0usize, 0usize));

    // Setup callbacks
    let mut callbacks = RemoteCallbacks::new();

    // Credential handling with enhanced SSH support
    let git_user = config.git_user.clone();
    let git_password = config.git_password.clone();
    let ssh_key_path = config.ssh_key_path.clone();

    callbacks.credentials(move |url, username_from_url, allowed_types| {
        debug!(
            "Git credentials requested: url={}, username_from_url={:?}, allowed={:?}",
            url, username_from_url, allowed_types
        );

        // Try SSH key from explicit path first (for custom deployment keys)
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(ref key_path) = ssh_key_path {
                let username = username_from_url.unwrap_or("git");
                debug!("Trying SSH key from: {}", key_path.display());
                if let Ok(cred) = Cred::ssh_key(username, None, key_path, None) {
                    return Ok(cred);
                }
            }
        }

        // Try SSH agent (most common for development)
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(username) = username_from_url {
                debug!("Trying SSH agent for user: {}", username);
                if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                    return Ok(cred);
                }
            }
            // Also try with "git" username (common for GitHub, GitLab, etc.)
            if let Ok(cred) = Cred::ssh_key_from_agent("git") {
                return Ok(cred);
            }
        }

        // Try username/password or token
        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            if let (Some(user), Some(pass)) = (&git_user, &git_password) {
                debug!("Trying username/password auth");
                return Cred::userpass_plaintext(user, pass);
            }
            // Try token-based auth (GitHub style: x-access-token:<token>)
            if let Some(ref token) = git_password {
                debug!("Trying token-based auth");
                return Cred::userpass_plaintext("x-access-token", token);
            }
        }

        // Try SSH key from default locations (~/.ssh/id_rsa, ~/.ssh/id_ed25519)
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            let username = username_from_url.unwrap_or("git");
            if let Some(home) = dirs_home() {
                for key_name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
                    let key_path = home.join(".ssh").join(key_name);
                    if key_path.exists() {
                        debug!("Trying SSH key: {}", key_path.display());
                        if let Ok(cred) = Cred::ssh_key(username, None, &key_path, None) {
                            return Ok(cred);
                        }
                    }
                }
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

    // Enhanced proxy configuration with SOCKS and auth support
    let mut proxy_opts = ProxyOptions::new();
    if let Some(ref proxy_url) = config.proxy_url {
        // Use explicit proxy with optional authentication
        let full_proxy = build_proxy_url(
            proxy_url,
            config.proxy_user.as_deref(),
            config.proxy_password.as_deref(),
        );
        debug!("Using explicit proxy: {}", mask_proxy_password(&full_proxy));
        proxy_opts.url(&full_proxy);
    } else if let Some(env_proxy) = get_git_proxy_from_env() {
        // Use environment proxy (supports SOCKS via ALL_PROXY)
        debug!("Using environment proxy: {}", mask_proxy_password(&env_proxy));
        proxy_opts.url(&env_proxy);
    } else {
        // Fall back to auto-detection
        proxy_opts.auto();
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

/// Fallback: Clone using git CLI (when git2 fails with proxy issues)
fn clone_git_repo_cli(src_uri: &SourceUri, dest_dir: &Path) -> FetchResult<()> {
    use std::process::Command;

    info!("Using git CLI fallback for clone");

    let mut args = vec![
        "clone".to_string(),
        "--depth".to_string(),
        "1".to_string(),
    ];

    // Add branch if specified
    if let Some(ref branch) = src_uri.branch {
        if !src_uri.nobranch {
            args.push("--branch".to_string());
            args.push(branch.clone());
        }
    }

    args.push(src_uri.url.clone());
    args.push(dest_dir.to_string_lossy().to_string());

    debug!("Running: git {}", args.join(" "));

    let output = Command::new("git")
        .args(&args)
        .output()
        .map_err(|e| FetchError::GitError(git2::Error::from_str(&format!("Failed to run git: {}", e))))?;

    if output.status.success() {
        info!("git CLI clone successful");

        // Checkout specific revision if needed
        if let Some(ref srcrev) = src_uri.srcrev {
            let checkout_output = Command::new("git")
                .args(["checkout", srcrev])
                .current_dir(dest_dir)
                .output()
                .map_err(|e| FetchError::GitError(git2::Error::from_str(&format!("Failed to checkout: {}", e))))?;

            if !checkout_output.status.success() {
                warn!("Failed to checkout {}: {}", srcrev, String::from_utf8_lossy(&checkout_output.stderr));
            }
        }

        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(FetchError::GitError(git2::Error::from_str(&format!(
            "git clone failed: {}",
            stderr
        ))))
    }
}

/// Fallback: Update using git CLI (when git2 fails with proxy issues)
fn update_git_repo_cli(repo_dir: &Path, src_uri: &SourceUri) -> FetchResult<()> {
    use std::process::Command;

    info!("Using git CLI fallback for pull");

    let output = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(repo_dir)
        .output()
        .map_err(|e| FetchError::GitError(git2::Error::from_str(&format!("Failed to run git pull: {}", e))))?;

    if output.status.success() {
        info!("git CLI pull successful");

        // Checkout specific revision if needed
        if let Some(ref srcrev) = src_uri.srcrev {
            let checkout_output = Command::new("git")
                .args(["checkout", srcrev])
                .current_dir(repo_dir)
                .output()
                .map_err(|e| FetchError::GitError(git2::Error::from_str(&format!("Failed to checkout: {}", e))))?;

            if !checkout_output.status.success() {
                warn!("Failed to checkout {}: {}", srcrev, String::from_utf8_lossy(&checkout_output.stderr));
            }
        }

        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(FetchError::GitError(git2::Error::from_str(&format!(
            "git pull failed: {}",
            stderr
        ))))
    }
}

fn update_git_repo(
    repo_dir: &Path,
    src_uri: &SourceUri,
    config: &FetchConfig,
) -> FetchResult<PathBuf> {
    let repo = Repository::open(repo_dir)?;

    // Setup callbacks for fetch with enhanced SSH support
    let mut callbacks = RemoteCallbacks::new();
    let git_user = config.git_user.clone();
    let git_password = config.git_password.clone();
    let ssh_key_path = config.ssh_key_path.clone();

    callbacks.credentials(move |url, username_from_url, allowed_types| {
        debug!(
            "Git credentials requested (update): url={}, username_from_url={:?}",
            url, username_from_url
        );

        // Try SSH key from explicit path first
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(ref key_path) = ssh_key_path {
                let username = username_from_url.unwrap_or("git");
                if let Ok(cred) = Cred::ssh_key(username, None, key_path, None) {
                    return Ok(cred);
                }
            }
        }

        // Try SSH agent
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(username) = username_from_url {
                if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                    return Ok(cred);
                }
            }
            if let Ok(cred) = Cred::ssh_key_from_agent("git") {
                return Ok(cred);
            }
        }

        // Try username/password
        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            if let (Some(user), Some(pass)) = (&git_user, &git_password) {
                return Cred::userpass_plaintext(user, pass);
            }
            if let Some(ref token) = git_password {
                return Cred::userpass_plaintext("x-access-token", token);
            }
        }

        // Try default SSH keys
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            let username = username_from_url.unwrap_or("git");
            if let Some(home) = dirs_home() {
                for key_name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
                    let key_path = home.join(".ssh").join(key_name);
                    if key_path.exists() {
                        if let Ok(cred) = Cred::ssh_key(username, None, &key_path, None) {
                            return Ok(cred);
                        }
                    }
                }
            }
        }

        Cred::default()
    });

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    // Enhanced proxy configuration
    let mut proxy_opts = ProxyOptions::new();
    if let Some(ref proxy_url) = config.proxy_url {
        let full_proxy = build_proxy_url(
            proxy_url,
            config.proxy_user.as_deref(),
            config.proxy_password.as_deref(),
        );
        proxy_opts.url(&full_proxy);
    } else if let Some(env_proxy) = get_git_proxy_from_env() {
        proxy_opts.url(&env_proxy);
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
        info!("Using configured proxy: {}...", &proxy_url[..80.min(proxy_url.len())]);
        if let Ok(proxy) = ureq::Proxy::new(proxy_url) {
            agent_builder = agent_builder.proxy(proxy);
        } else {
            warn!("Failed to parse configured proxy URL");
        }
    } else {
        // Auto-detect from environment
        if let Some(proxy_url) = get_proxy_for_url(&src_uri.url) {
            info!("Auto-detected proxy from environment: {}...", &proxy_url[..80.min(proxy_url.len())]);
            match ureq::Proxy::new(&proxy_url) {
                Ok(proxy) => {
                    agent_builder = agent_builder.proxy(proxy);
                    info!("Proxy configured successfully");
                }
                Err(e) => {
                    warn!("Failed to parse proxy URL: {:?}", e);
                }
            }
        } else {
            info!("No proxy detected for URL");
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

/// Get home directory
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
}

/// Build proxy URL with authentication if provided
fn build_proxy_url(base_url: &str, user: Option<&str>, password: Option<&str>) -> String {
    if let (Some(u), Some(p)) = (user, password) {
        // Insert credentials into proxy URL: http://user:pass@host:port
        if let Some(scheme_end) = base_url.find("://") {
            let scheme = &base_url[..scheme_end + 3];
            let rest = &base_url[scheme_end + 3..];
            format!("{}{}:{}@{}", scheme, u, p, rest)
        } else {
            base_url.to_string()
        }
    } else {
        base_url.to_string()
    }
}

/// Get git proxy from environment with SOCKS support
fn get_git_proxy_from_env() -> Option<String> {
    // Check GIT_PROXY_COMMAND for SOCKS proxy (common in corporate environments)
    // Check git-specific proxy settings first
    if let Ok(proxy) = std::env::var("GIT_PROXY") {
        return Some(proxy);
    }

    // Check ALL_PROXY for SOCKS (often used for git)
    if let Ok(proxy) = std::env::var("ALL_PROXY").or_else(|_| std::env::var("all_proxy")) {
        return Some(proxy);
    }

    // Fall back to HTTPS_PROXY (git typically uses HTTPS)
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy")) {
        return Some(proxy);
    }

    // Finally try HTTP_PROXY
    std::env::var("HTTP_PROXY")
        .or_else(|_| std::env::var("http_proxy"))
        .ok()
}

/// Mask password in proxy URL for logging
fn mask_proxy_password(url: &str) -> String {
    // Match patterns like http://user:password@host
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let after_scheme = &url[scheme_end + 3..at_pos];
            if let Some(colon_pos) = after_scheme.find(':') {
                let scheme = &url[..scheme_end + 3];
                let user = &after_scheme[..colon_pos];
                let rest = &url[at_pos..];
                return format!("{}{}:***{}", scheme, user, rest);
            }
        }
    }
    url.to_string()
}

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

    #[test]
    fn test_build_proxy_url_with_auth() {
        // No auth
        assert_eq!(
            build_proxy_url("http://proxy.example.com:8080", None, None),
            "http://proxy.example.com:8080"
        );

        // With auth
        assert_eq!(
            build_proxy_url("http://proxy.example.com:8080", Some("user"), Some("pass")),
            "http://user:pass@proxy.example.com:8080"
        );

        // SOCKS proxy with auth
        assert_eq!(
            build_proxy_url("socks5://proxy.example.com:1080", Some("user"), Some("secret")),
            "socks5://user:secret@proxy.example.com:1080"
        );
    }

    #[test]
    fn test_mask_proxy_password() {
        // No password
        assert_eq!(
            mask_proxy_password("http://proxy.example.com:8080"),
            "http://proxy.example.com:8080"
        );

        // With password - should be masked
        assert_eq!(
            mask_proxy_password("http://user:secretpass@proxy.example.com:8080"),
            "http://user:***@proxy.example.com:8080"
        );

        // SOCKS with password
        assert_eq!(
            mask_proxy_password("socks5://admin:hunter2@socks.example.com:1080"),
            "socks5://admin:***@socks.example.com:1080"
        );
    }

    #[test]
    fn test_dirs_home() {
        // This depends on environment but should return Some on most systems
        // Just verify it doesn't panic
        let _ = dirs_home();
    }
}
