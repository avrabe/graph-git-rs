//! Package management (RPM/DEB generation)

use serde::{Serialize, Deserialize};
use std::path::{Path, PathBuf};

/// Package format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFormat {
    RPM,
    DEB,
    IPK,
}

/// Package metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub description: String,
    pub dependencies: Vec<String>,
    pub files: Vec<PathBuf>,
}

/// Package builder
pub struct PackageBuilder {
    metadata: PackageMetadata,
    format: PackageFormat,
}

impl PackageBuilder {
    pub fn new(metadata: PackageMetadata, format: PackageFormat) -> Self {
        Self { metadata, format }
    }

    /// Build package
    pub fn build(&self, output_dir: &Path) -> std::io::Result<PathBuf> {
        match self.format {
            PackageFormat::RPM => self.build_rpm(output_dir),
            PackageFormat::DEB => self.build_deb(output_dir),
            PackageFormat::IPK => self.build_ipk(output_dir),
        }
    }

    fn build_rpm(&self, output_dir: &Path) -> std::io::Result<PathBuf> {
        // TODO: RPM package generation
        // 1. Create .spec file
        // 2. Build with rpmbuild
        // 3. Sign package
        Ok(output_dir.join(format!("{}-{}.rpm",
            self.metadata.name, self.metadata.version)))
    }

    fn build_deb(&self, output_dir: &Path) -> std::io::Result<PathBuf> {
        // TODO: DEB package generation
        // 1. Create DEBIAN/control
        // 2. Build with dpkg-deb
        // 3. Sign package
        Ok(output_dir.join(format!("{}_{}.deb",
            self.metadata.name, self.metadata.version)))
    }

    fn build_ipk(&self, output_dir: &Path) -> std::io::Result<PathBuf> {
        // TODO: IPK package generation (OpenWrt format)
        Ok(output_dir.join(format!("{}_{}.ipk",
            self.metadata.name, self.metadata.version)))
    }

    /// Generate control file (DEB)
    pub fn generate_control(&self) -> String {
        format!(r#"Package: {}
Version: {}
Architecture: {}
Description: {}
Depends: {}
"#,
            self.metadata.name,
            self.metadata.version,
            self.metadata.arch,
            self.metadata.description,
            self.metadata.dependencies.join(", "),
        )
    }

    /// Generate spec file (RPM)
    pub fn generate_spec(&self) -> String {
        format!(r#"Name: {}
Version: {}
Release: 1
Summary: {}
License: Unknown
BuildArch: {}

%description
{}

%files
{}
"#,
            self.metadata.name,
            self.metadata.version,
            self.metadata.description,
            self.metadata.arch,
            self.metadata.description,
            self.metadata.files.iter()
                .map(|f| f.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}
