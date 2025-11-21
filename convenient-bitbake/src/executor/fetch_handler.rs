//! Real source fetching for do_fetch tasks
//!
//! This module implements actual source downloading using system tools (git, wget, curl)
//! rather than trying to replicate BitBake's complex Python fetch2 module.
//!
//! ## Strategy for 80% BitBake Compatibility
//!
//! BitBake uses `bb.fetch2.Fetch()` which is complex Python code handling many protocols.
//! For an 80% solution, we use system tools directly:
//!
//! - **Git URLs**: Use `git clone` command
//! - **HTTP/HTTPS**: Use `wget` or `curl`
//! - **File URLs**: Use `cp` command
//!
//! This covers the vast majority of real-world Yocto/OpenEmbedded recipes.
//!
//! ## What We Don't Support (20%)
//!
//! - SVN, CVS, Perforce (rarely used)
//! - BitBake's mirror handling
//! - Premirror/mirror fallback
//! - Git submodules (could add later)
//! - Complex git protocol negotiation
//!
//! ## Example
//!
//! ```rust
//! use convenient_bitbake::{SourceUri, UriScheme};
//! use convenient_bitbake::executor::fetch_handler::fetch_source;
//! use std::path::Path;
//!
//! let src_uri = SourceUri {
//!     scheme: UriScheme::Git,
//!     url: "https://github.com/torvalds/linux.git".to_string(),
//!     branch: Some("master".to_string()),
//!     srcrev: Some("v6.1".to_string()),
//!     ..Default::default()
//! };
//!
//! let downloads_dir = Path::new("/tmp/downloads");
//! // fetch_source(&src_uri, downloads_dir)?;
//! ```

use crate::{SourceUri, UriScheme};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during source fetching
#[derive(Debug, Error)]
pub enum FetchError {
    #[error("Unsupported URI scheme: {0:?}")]
    UnsupportedScheme(UriScheme),

    #[error("Git clone failed: {0}")]
    GitCloneFailed(String),

    #[error("Git checkout failed: {0}")]
    GitCheckoutFailed(String),

    #[error("HTTP download failed: {0}")]
    HttpDownloadFailed(String),

    #[error("File copy failed: {0}")]
    FileCopyFailed(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Required tool not found: {0}")]
    ToolNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

/// Result type for fetch operations
pub type FetchResult<T> = Result<T, FetchError>;

/// Download source from SRC_URI to downloads directory
///
/// This is the main entry point for fetching sources. It determines the
/// appropriate fetcher based on the URI scheme and delegates to the
/// specialized handler.
///
/// # Arguments
///
/// * `src_uri` - Parsed SRC_URI from recipe
/// * `downloads_dir` - Directory to store downloaded files (typically DL_DIR)
///
/// # Returns
///
/// Path to the downloaded file or cloned repository
pub fn fetch_source(src_uri: &SourceUri, downloads_dir: &Path) -> FetchResult<PathBuf> {
    info!("Fetching source: {}", src_uri.url);
    debug!("  Scheme: {:?}", src_uri.scheme);
    debug!("  Branch: {:?}", src_uri.branch);
    debug!("  Tag: {:?}", src_uri.tag);
    debug!("  SRCREV: {:?}", src_uri.srcrev);

    // Ensure downloads directory exists
    fs::create_dir_all(downloads_dir)?;

    match src_uri.scheme {
        UriScheme::Git | UriScheme::GitSubmodule => fetch_git(src_uri, downloads_dir),
        UriScheme::Http | UriScheme::Https => fetch_http(src_uri, downloads_dir),
        UriScheme::File => fetch_file(src_uri, downloads_dir),
        _ => Err(FetchError::UnsupportedScheme(src_uri.scheme.clone())),
    }
}

/// Fetch source from Git repository
///
/// Uses `git clone` command with appropriate options based on SRC_URI parameters.
///
/// ## Example SRC_URI patterns handled:
///
/// - `git://github.com/foo/bar.git;branch=main`
/// - `git://git.yoctoproject.org/poky;protocol=https;branch=kirkstone`
/// - `git://github.com/busybox/busybox.git;tag=1_35_0;protocol=https`
fn fetch_git(src_uri: &SourceUri, downloads_dir: &Path) -> FetchResult<PathBuf> {
    // Check git is available
    check_tool_exists("git")?;

    // Determine repository name for destination directory
    let repo_name = extract_repo_name(&src_uri.url)?;
    let dest_dir = downloads_dir.join(&repo_name);

    // If already cloned, fetch updates
    if dest_dir.join(".git").exists() {
        info!("Repository already exists, fetching updates: {}", dest_dir.display());
        return update_git_repo(&dest_dir, src_uri);
    }

    info!("Cloning git repository: {}", src_uri.url);

    // Build git clone command
    let mut cmd = Command::new("git");
    cmd.arg("clone");

    // Use shallow clone for speed (unless full history needed)
    if src_uri.srcrev.is_none() && src_uri.tag.is_none() {
        cmd.args(["--depth", "1"]);
    }

    // Clone specific branch if specified
    if let Some(ref branch) = src_uri.branch
        && !src_uri.nobranch {
            cmd.args(["--branch", branch]);
        }

    // Add URL and destination
    cmd.arg(&src_uri.url);
    cmd.arg(&dest_dir);

    debug!("Running: {:?}", cmd);

    let output = cmd.output()?;

    if !output.status.success() {
        return Err(FetchError::GitCloneFailed(
            String::from_utf8_lossy(&output.stderr).to_string()
        ));
    }

    info!("✓ Cloned successfully to {}", dest_dir.display());

    // Checkout specific revision if SRCREV specified
    if let Some(ref srcrev) = src_uri.srcrev {
        checkout_git_revision(&dest_dir, srcrev)?;
    }
    // Or checkout specific tag if specified
    else if let Some(ref tag) = src_uri.tag {
        checkout_git_revision(&dest_dir, tag)?;
    }

    Ok(dest_dir)
}

/// Update existing git repository
fn update_git_repo(repo_dir: &Path, src_uri: &SourceUri) -> FetchResult<PathBuf> {
    debug!("Fetching updates for {}", repo_dir.display());

    let mut cmd = Command::new("git");
    cmd.current_dir(repo_dir);
    cmd.args(["fetch", "origin"]);

    if let Some(ref branch) = src_uri.branch {
        cmd.arg(branch);
    }

    let output = cmd.output()?;

    if !output.status.success() {
        warn!("Git fetch failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    // Checkout requested revision
    if let Some(ref srcrev) = src_uri.srcrev {
        checkout_git_revision(repo_dir, srcrev)?;
    } else if let Some(ref tag) = src_uri.tag {
        checkout_git_revision(repo_dir, tag)?;
    } else if let Some(ref branch) = src_uri.branch {
        checkout_git_revision(repo_dir, &format!("origin/{branch}"))?;
    }

    Ok(repo_dir.to_path_buf())
}

/// Checkout specific git revision/tag/branch
fn checkout_git_revision(repo_dir: &Path, revision: &str) -> FetchResult<()> {
    info!("Checking out revision: {}", revision);

    let output = Command::new("git")
        .current_dir(repo_dir)
        .args(["checkout", revision])
        .output()?;

    if !output.status.success() {
        return Err(FetchError::GitCheckoutFailed(
            String::from_utf8_lossy(&output.stderr).to_string()
        ));
    }

    info!("✓ Checked out {}", revision);
    Ok(())
}

/// Fetch file via HTTP/HTTPS
///
/// Uses `wget` or `curl` (whichever is available) to download files.
///
/// ## Example SRC_URI patterns:
///
/// - `https://busybox.net/downloads/busybox-${PV}.tar.bz2`
/// - `http://ftp.gnu.org/gnu/bash/bash-5.2.tar.gz`
fn fetch_http(src_uri: &SourceUri, downloads_dir: &Path) -> FetchResult<PathBuf> {
    // Determine filename from URL
    let filename = extract_filename(&src_uri.url)?;
    let dest_file = downloads_dir.join(&filename);

    // Skip if already downloaded
    if dest_file.exists() {
        info!("File already exists: {}", dest_file.display());
        return Ok(dest_file);
    }

    info!("Downloading: {}", src_uri.url);

    // Try wget first, fall back to curl
    let result = if check_tool_exists("wget").is_ok() {
        download_with_wget(&src_uri.url, &dest_file)
    } else if check_tool_exists("curl").is_ok() {
        download_with_curl(&src_uri.url, &dest_file)
    } else {
        return Err(FetchError::ToolNotFound(
            "Neither wget nor curl found".to_string()
        ));
    };

    result?;
    info!("✓ Downloaded to {}", dest_file.display());
    Ok(dest_file)
}

/// Download file using wget
fn download_with_wget(url: &str, dest: &Path) -> FetchResult<()> {
    let output = Command::new("wget")
        .args([
            "-O", dest.to_str().unwrap(),
            "--no-check-certificate",  // Some build servers use self-signed certs
            url
        ])
        .output()?;

    if !output.status.success() {
        return Err(FetchError::HttpDownloadFailed(
            String::from_utf8_lossy(&output.stderr).to_string()
        ));
    }

    Ok(())
}

/// Download file using curl
fn download_with_curl(url: &str, dest: &Path) -> FetchResult<()> {
    let output = Command::new("curl")
        .args([
            "-o", dest.to_str().unwrap(),
            "-L",  // Follow redirects
            "--insecure",  // Some build servers use self-signed certs
            url
        ])
        .output()?;

    if !output.status.success() {
        return Err(FetchError::HttpDownloadFailed(
            String::from_utf8_lossy(&output.stderr).to_string()
        ));
    }

    Ok(())
}

/// Copy local file
///
/// Handles `file://` URLs by copying to downloads directory.
fn fetch_file(src_uri: &SourceUri, downloads_dir: &Path) -> FetchResult<PathBuf> {
    // Remove file:// prefix
    let src_path = src_uri.url.strip_prefix("file://")
        .unwrap_or(&src_uri.url);

    let src = Path::new(src_path);
    let filename = src.file_name()
        .ok_or_else(|| FetchError::InvalidUrl(format!("No filename in: {}", src_uri.url)))?;

    let dest = downloads_dir.join(filename);

    if dest.exists() {
        info!("File already exists: {}", dest.display());
        return Ok(dest);
    }

    info!("Copying file: {} -> {}", src.display(), dest.display());

    fs::copy(src, &dest)
        .map_err(|e| FetchError::FileCopyFailed(e.to_string()))?;

    info!("✓ Copied to {}", dest.display());
    Ok(dest)
}

/// Extract repository name from git URL
///
/// Examples:
/// - `https://github.com/foo/bar.git` → `bar`
/// - `git://git.yoctoproject.org/poky` → `poky`
fn extract_repo_name(url: &str) -> FetchResult<String> {
    let url_clean = url.trim_end_matches('/');

    let name = url_clean
        .rsplit('/')
        .next()
        .ok_or_else(|| FetchError::InvalidUrl(format!("Cannot extract repo name from: {url}")))?
        .trim_end_matches(".git");

    Ok(name.to_string())
}

/// Extract filename from HTTP URL
///
/// Examples:
/// - `https://example.com/file.tar.gz` → `file.tar.gz`
/// - `http://example.com/path/to/archive.zip` → `archive.zip`
fn extract_filename(url: &str) -> FetchResult<String> {
    let url_clean = url.trim_end_matches('/');

    let name = url_clean
        .rsplit('/')
        .next()
        .ok_or_else(|| FetchError::InvalidUrl(format!("Cannot extract filename from: {url}")))?;

    // Remove query parameters if present
    let name_clean = name.split('?').next().unwrap_or(name);

    if name_clean.is_empty() {
        return Err(FetchError::InvalidUrl(format!("Empty filename in: {url}")));
    }

    Ok(name_clean.to_string())
}

/// Check if a command-line tool exists
fn check_tool_exists(tool: &str) -> FetchResult<()> {
    let output = Command::new("which")
        .arg(tool)
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(FetchError::ToolNotFound(tool.to_string()))
    }
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
        assert_eq!(
            extract_repo_name("https://github.com/torvalds/linux.git/").unwrap(),
            "linux"
        );
    }

    #[test]
    fn test_extract_filename() {
        assert_eq!(
            extract_filename("https://example.com/file.tar.gz").unwrap(),
            "file.tar.gz"
        );
        assert_eq!(
            extract_filename("http://example.com/path/to/archive.zip?query=1").unwrap(),
            "archive.zip"
        );
    }
}
