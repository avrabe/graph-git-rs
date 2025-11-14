//! BitBake task representation

use std::hash::Hash;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Represents a BitBake task extracted from a recipe
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitBakeTask {
    /// Recipe name (e.g., "busybox")
    pub recipe: String,

    /// Task name (e.g., "do_fetch", "do_compile")
    pub task: String,

    /// Package name with version
    pub pn: String,

    /// Package version
    pub pv: String,

    /// Input files/dependencies
    pub inputs: Vec<String>,

    /// Output files/artifacts
    pub outputs: Vec<String>,

    /// Working directory
    pub workdir: PathBuf,

    /// Shell command to execute
    pub command: String,
}

impl BitBakeTask {
    /// Get the qualified name of this task (recipe:task)
    pub fn qualified_name(&self) -> String {
        format!("{}:{}", self.recipe, self.task)
    }

    /// Get the stamp file path for this task
    pub fn stamp_file(&self) -> PathBuf {
        self.workdir.join(format!("{}.{}.stamp", self.pn, self.task))
    }

    /// Create a new BitBake task
    pub fn new(
        recipe: impl Into<String>,
        task: impl Into<String>,
        pn: impl Into<String>,
        pv: impl Into<String>,
        inputs: Vec<String>,
        outputs: Vec<String>,
        workdir: PathBuf,
        command: impl Into<String>,
    ) -> Self {
        Self {
            recipe: recipe.into(),
            task: task.into(),
            pn: pn.into(),
            pv: pv.into(),
            inputs,
            outputs,
            workdir,
            command: command.into(),
        }
    }
}
