//! Python bridge modules for BitBake execution
//!
//! This module provides the bridge between RustPython-executed BitBake Python code
//! and our fast Rust backend implementations.
//!
//! ## Architecture
//!
//! ```text
//! Python (in recipe)  →  RustPython VM  →  bb.fetch2 (Python)  →  Rust fetch_handler
//!                                         ↑
//!                                    This module
//! ```
//!
//! ## Modules Provided
//!
//! - `bb.data` - DataStore for variable storage (in python_executor.rs)
//! - `bb.fetch2` - Fetch module that delegates to Rust
//! - `bb.utils` - Utility functions (in python_executor.rs)

use rustpython::vm::{
    builtins::PyListRef,
    pyclass, pymodule, PyObjectRef, PyPayload, PyResult, VirtualMachine,
};
use std::path::PathBuf;
use tracing::{debug, info};

use crate::executor::fetch_handler;
use crate::{SourceUri, UriScheme};

/// Module containing bb.fetch2 for source fetching
#[pymodule]
pub(crate) mod bb_fetch2 {
    use super::*;

    /// Fetch class that downloads sources
    ///
    /// This mimics BitBake's bb.fetch2.Fetch class but delegates
    /// to our Rust fetch_handler for actual downloading.
    ///
    /// # Example (from BitBake recipe)
    ///
    /// ```python
    /// python do_fetch() {
    ///     src_uri = (d.getVar('SRC_URI') or "").split()
    ///     fetcher = bb.fetch2.Fetch(src_uri, d)
    ///     fetcher.download()
    /// }
    /// ```
    #[pyattr]
    #[pyclass(module = "bb.fetch2", name = "Fetch")]
    #[derive(Debug, Clone, PyPayload)]
    pub(super) struct Fetch {
        urls: Vec<String>,
        // Note: We don't actually use the DataStore yet,
        // but we accept it for API compatibility
        #[allow(dead_code)]
        datastore: PyObjectRef,
    }

    #[pyclass]
    impl Fetch {
        /// Create a new Fetch instance
        ///
        /// # Arguments
        /// * `urls` - List of SRC_URI strings
        /// * `d` - DataStore object (for compatibility)
        #[pymethod(magic)]
        fn __init__(
            _zelf: PyObjectRef,
            _urls: PyObjectRef,
            _d: PyObjectRef,
            _vm: &VirtualMachine,
        ) -> PyResult<()> {
            // Constructor called from Python - actual init happens in __new__
            Ok(())
        }

        /// Download all sources
        ///
        /// This is the main method called from do_fetch tasks.
        /// It delegates to our Rust fetch_handler for each URL.
        #[pymethod]
        fn download(&self, vm: &VirtualMachine) -> PyResult<()> {
            info!("bb.fetch2.Fetch.download() called with {} URLs", self.urls.len());

            // Get downloads directory from environment or use default
            let downloads_dir = std::env::var("DL_DIR")
                .unwrap_or_else(|_| "/tmp/downloads".to_string());
            let downloads_path = PathBuf::from(&downloads_dir);

            // Download each URL
            for url_str in &self.urls {
                debug!("Fetching: {}", url_str);

                // Parse the SRC_URI string
                let src_uri = match parse_src_uri(url_str) {
                    Ok(uri) => uri,
                    Err(e) => {
                        return Err(vm.new_value_error(format!(
                            "Failed to parse SRC_URI '{}': {}",
                            url_str, e
                        )));
                    }
                };

                // Call Rust fetch_handler
                match fetch_handler::fetch_source(&src_uri, &downloads_path) {
                    Ok(downloaded_path) => {
                        info!("✓ Downloaded: {}", downloaded_path.display());
                    }
                    Err(e) => {
                        return Err(vm.new_runtime_error(format!(
                            "Fetch failed for '{}': {}",
                            url_str, e
                        )));
                    }
                }
            }

            info!("✓ All sources fetched successfully");
            Ok(())
        }
    }

    /// Create Fetch instance from Python
    ///
    /// This is called when Python does: `bb.fetch2.Fetch(urls, d)`
    #[pyfunction]
    fn Fetch(urls: PyListRef, d: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        // Convert Python list to Rust Vec<String>
        let mut url_vec = Vec::new();
        for item in urls.borrow_vec().iter() {
            // Try to get string from Python object
            if let Ok(s) = item.try_to_value(vm) {
                url_vec.push(s);
            } else {
                return Err(vm.new_type_error("SRC_URI must be list of strings".to_string()));
            }
        }

        let fetch_instance = super::bb_fetch2::Fetch {
            urls: url_vec,
            datastore: d.clone(),
        };

        Ok(fetch_instance.into_pyobject(vm))
    }
}

/// Module containing bb package (top-level)
#[pymodule]
pub(crate) mod bb {
    use super::*;

    #[pyattr]
    fn fetch2(_vm: &VirtualMachine) -> PyObjectRef {
        // This will be set up in the interpreter init
        _vm.ctx.none()
    }

    #[pyattr]
    fn data(_vm: &VirtualMachine) -> PyObjectRef {
        // This will be set up in the interpreter init
        _vm.ctx.none()
    }

    #[pyattr]
    fn utils(_vm: &VirtualMachine) -> PyObjectRef {
        // This will be set up in the interpreter init
        _vm.ctx.none()
    }
}

/// Parse a SRC_URI string into our SourceUri struct
///
/// Handles formats like:
/// - `git://github.com/foo/bar.git;branch=main;protocol=https`
/// - `https://example.com/file.tar.gz`
/// - `file:///path/to/local/file`
fn parse_src_uri(uri_str: &str) -> Result<SourceUri, String> {
    // Split on semicolon to get URL and parameters
    let parts: Vec<&str> = uri_str.split(';').collect();
    let url = parts[0].trim();

    // Determine scheme
    let scheme = if url.starts_with("git://") || url.starts_with("git@") {
        UriScheme::Git
    } else if url.starts_with("https://") {
        UriScheme::Https
    } else if url.starts_with("http://") {
        UriScheme::Http
    } else if url.starts_with("file://") {
        UriScheme::File
    } else {
        UriScheme::Other(url.split("://").next().unwrap_or("unknown").to_string())
    };

    // Parse parameters
    let mut src_uri = SourceUri {
        raw: uri_str.to_string(),
        scheme,
        url: url.to_string(),
        protocol: None,
        branch: None,
        tag: None,
        srcrev: None,
        nobranch: false,
        destsuffix: None,
    };

    // Process parameters
    for param in parts.iter().skip(1) {
        let param = param.trim();
        if let Some((key, value)) = param.split_once('=') {
            match key {
                "protocol" => src_uri.protocol = Some(value.to_string()),
                "branch" => src_uri.branch = Some(value.to_string()),
                "tag" => src_uri.tag = Some(value.to_string()),
                "srcrev" | "rev" => src_uri.srcrev = Some(value.to_string()),
                "destsuffix" => src_uri.destsuffix = Some(value.to_string()),
                _ => {} // Ignore unknown parameters
            }
        } else if param == "nobranch" {
            src_uri.nobranch = true;
        }
    }

    Ok(src_uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_uri() {
        let uri = parse_src_uri("git://github.com/foo/bar.git;branch=main;protocol=https").unwrap();
        assert!(matches!(uri.scheme, UriScheme::Git));
        assert_eq!(uri.branch, Some("main".to_string()));
        assert_eq!(uri.protocol, Some("https".to_string()));
    }

    #[test]
    fn test_parse_https_uri() {
        let uri = parse_src_uri("https://example.com/file.tar.gz").unwrap();
        assert!(matches!(uri.scheme, UriScheme::Https));
        assert_eq!(uri.url, "https://example.com/file.tar.gz");
    }

    #[test]
    fn test_parse_file_uri() {
        let uri = parse_src_uri("file:///path/to/file.patch").unwrap();
        assert!(matches!(uri.scheme, UriScheme::File));
    }
}
