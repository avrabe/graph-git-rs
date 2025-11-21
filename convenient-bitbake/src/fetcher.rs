//! Source fetcher for BitBake recipes
//!
//! Handles downloading and unpacking sources from SRC_URI

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

#[derive(Debug)]
pub enum FetchError {
    Io(io::Error),
    Http(String),
    Unsupported(String),
}

impl From<io::Error> for FetchError {
    fn from(e: io::Error) -> Self {
        FetchError::Io(e)
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FetchError::Io(e) => write!(f, "IO error: {}", e),
            FetchError::Http(e) => write!(f, "HTTP error: {}", e),
            FetchError::Unsupported(s) => write!(f, "Unsupported: {}", s),
        }
    }
}

impl std::error::Error for FetchError {}

/// Parse SRC_URI and extract download URLs
pub fn parse_src_uri(src_uri: &str) -> Vec<(String, String)> {
    let mut sources = Vec::new();

    // SRC_URI can have multiple entries separated by whitespace
    // Example: "https://example.com/file.tar.gz;name=tarball file://patch.patch"
    for entry in src_uri.split_whitespace() {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        // Split on semicolon to separate URL from parameters
        let parts: Vec<&str> = entry.split(';').collect();
        let url = parts[0];

        // Extract name parameter if present
        let mut name = String::new();
        for param in parts.iter().skip(1) {
            if param.starts_with("name=") {
                name = param.strip_prefix("name=").unwrap_or("").to_string();
            }
        }

        // Only process http/https URLs for now
        if url.starts_with("http://") || url.starts_with("https://") {
            sources.push((url.to_string(), name));
        } else if url.starts_with("file://") {
            // file:// URLs are patches/local files, skip for now
            debug!("Skipping file:// URL: {}", url);
        } else if url.starts_with("git://") {
            info!("git:// URL found but not yet implemented: {}", url);
        }
    }

    sources
}

/// Fetch source from URL to download directory
pub fn fetch_source(url: &str, download_dir: &Path) -> Result<PathBuf, FetchError> {
    fs::create_dir_all(download_dir)?;

    // Extract filename from URL
    let filename = url.rsplit('/').next()
        .ok_or_else(|| FetchError::Http("Invalid URL".to_string()))?;

    let dest_path = download_dir.join(filename);

    // Check if already downloaded
    if dest_path.exists() {
        info!("Source already downloaded: {}", dest_path.display());
        return Ok(dest_path);
    }

    info!("Fetching {} to {}", url, dest_path.display());

    // Download using ureq (blocking HTTP client)
    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| FetchError::Http(format!("Failed to fetch {}: {}", url, e)))?;

    // Write to temp file first
    let temp_path = dest_path.with_extension("tmp");
    let mut file = fs::File::create(&temp_path)?;
    let mut reader = response.into_reader();
    io::copy(&mut reader, &mut file)?;

    // Rename to final destination
    fs::rename(&temp_path, &dest_path)?;

    info!("Downloaded: {}", dest_path.display());
    Ok(dest_path)
}

/// Unpack archive to destination directory
pub fn unpack_source(archive_path: &Path, dest_dir: &Path) -> Result<(), FetchError> {
    fs::create_dir_all(dest_dir)?;

    info!("Unpacking {} to {}", archive_path.display(), dest_dir.display());

    let archive_name = archive_path.to_string_lossy();

    // Detect archive type by extension
    if archive_name.ends_with(".tar.gz") || archive_name.ends_with(".tgz") {
        unpack_tar_gz(archive_path, dest_dir)?;
    } else if archive_name.ends_with(".tar.bz2") || archive_name.ends_with(".tbz2") {
        unpack_tar_bz2(archive_path, dest_dir)?;
    } else if archive_name.ends_with(".tar.xz") {
        unpack_tar_xz(archive_path, dest_dir)?;
    } else if archive_name.ends_with(".tar") {
        unpack_tar(archive_path, dest_dir)?;
    } else {
        return Err(FetchError::Unsupported(format!(
            "Unsupported archive format: {}",
            archive_name
        )));
    }

    info!("Unpacked successfully");
    Ok(())
}

/// Unpack .tar.gz archive
fn unpack_tar_gz(archive_path: &Path, dest_dir: &Path) -> Result<(), FetchError> {
    let file = fs::File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest_dir)?;
    Ok(())
}

/// Unpack .tar.bz2 archive
fn unpack_tar_bz2(archive_path: &Path, dest_dir: &Path) -> Result<(), FetchError> {
    let file = fs::File::open(archive_path)?;
    let decoder = bzip2::read::BzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest_dir)?;
    Ok(())
}

/// Unpack .tar.xz archive
fn unpack_tar_xz(archive_path: &Path, dest_dir: &Path) -> Result<(), FetchError> {
    let file = fs::File::open(archive_path)?;
    let decoder = xz2::read::XzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest_dir)?;
    Ok(())
}

/// Unpack plain .tar archive
fn unpack_tar(archive_path: &Path, dest_dir: &Path) -> Result<(), FetchError> {
    let file = fs::File::open(archive_path)?;
    let mut archive = tar::Archive::new(file);
    archive.unpack(dest_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_src_uri() {
        let src_uri = "https://example.com/file-1.0.tar.gz;name=tarball file://patch.patch";
        let sources = parse_src_uri(src_uri);

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].0, "https://example.com/file-1.0.tar.gz");
        assert_eq!(sources[0].1, "tarball");
    }

    #[test]
    fn test_parse_src_uri_multiple() {
        let src_uri = "https://example.com/file1.tar.gz https://example.com/file2.tar.gz";
        let sources = parse_src_uri(src_uri);

        assert_eq!(sources.len(), 2);
    }
}
