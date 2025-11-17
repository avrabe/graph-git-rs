//! Real-world Poky/Yocto integration testing

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Poky integration tester
pub struct PokyTester {
    poky_path: PathBuf,
    build_dir: PathBuf,
}

impl PokyTester {
    pub fn new(poky_path: impl Into<PathBuf>) -> Self {
        let poky_path = poky_path.into();
        let build_dir = poky_path.join("build");
        Self { poky_path, build_dir }
    }

    /// Clone Poky repository
    pub fn clone_poky(target_dir: &Path) -> std::io::Result<Self> {
        println!("Cloning Poky repository...");
        let start = Instant::now();

        let status = Command::new("git")
            .args(&[
                "clone",
                "--depth", "1",
                "--branch", "kirkstone",
                "https://git.yoctoproject.org/git/poky",
                target_dir.to_str().unwrap(),
            ])
            .status()?;

        if !status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to clone Poky",
            ));
        }

        let elapsed = start.elapsed();
        println!("✓ Cloned Poky in {:.2}s", elapsed.as_secs_f64());

        Ok(Self::new(target_dir))
    }

    /// Find all recipe files
    pub fn find_recipes(&self) -> std::io::Result<Vec<PathBuf>> {
        use walkdir::WalkDir;

        let mut recipes = Vec::new();

        for entry in WalkDir::new(&self.poky_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("bb") {
                recipes.push(path.to_path_buf());
            }
        }

        Ok(recipes)
    }

    /// Test recipe parsing
    pub fn test_parsing(&self) -> std::io::Result<ParsingStats> {
        let recipes = self.find_recipes()?;
        let total = recipes.len();
        let start = Instant::now();

        let mut parsed = 0;
        let mut failed = 0;

        for recipe in &recipes {
            // TODO: Actually parse with our parser
            // For now, just count
            if recipe.exists() {
                parsed += 1;
            } else {
                failed += 1;
            }
        }

        let elapsed = start.elapsed();

        Ok(ParsingStats {
            total,
            parsed,
            failed,
            duration_s: elapsed.as_secs_f64(),
        })
    }

    /// Test building a specific recipe
    pub fn test_build(&self, recipe: &str) -> std::io::Result<BuildStats> {
        println!("Building recipe: {}", recipe);
        let start = Instant::now();

        // TODO: Integrate with our build system
        // For now, simulate
        std::thread::sleep(std::time::Duration::from_millis(100));

        let elapsed = start.elapsed();

        Ok(BuildStats {
            recipe: recipe.to_string(),
            success: true,
            duration_s: elapsed.as_secs_f64(),
            cache_hits: 0,
            cache_misses: 0,
        })
    }
}

/// Parsing statistics
#[derive(Debug, Clone)]
pub struct ParsingStats {
    pub total: usize,
    pub parsed: usize,
    pub failed: usize,
    pub duration_s: f64,
}

impl ParsingStats {
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 { 0.0 } else {
            (self.parsed as f64 / self.total as f64) * 100.0
        }
    }

    pub fn recipes_per_sec(&self) -> f64 {
        if self.duration_s == 0.0 { 0.0 } else {
            self.parsed as f64 / self.duration_s
        }
    }
}

/// Build statistics
#[derive(Debug, Clone)]
pub struct BuildStats {
    pub recipe: String,
    pub success: bool,
    pub duration_s: f64,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

impl BuildStats {
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 { 0.0 } else {
            (self.cache_hits as f64 / total as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing_stats() {
        let stats = ParsingStats {
            total: 100,
            parsed: 95,
            failed: 5,
            duration_s: 1.0,
        };

        assert_eq!(stats.success_rate(), 95.0);
        assert_eq!(stats.recipes_per_sec(), 95.0);
    }

    #[test]
    fn test_build_stats() {
        let stats = BuildStats {
            recipe: "busybox".to_string(),
            success: true,
            duration_s: 10.0,
            cache_hits: 80,
            cache_misses: 20,
        };

        assert_eq!(stats.cache_hit_rate(), 80.0);
    }
}
