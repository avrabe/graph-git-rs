//! Task graph builder from kas configuration

use crate::task::BitBakeTask;
use crate::Result;
use convenient_graph::DAG;
use convenient_kas::include_graph::KasIncludeGraph;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Builds task dependency graphs from kas configuration
pub struct TaskGraphBuilder {
    kas_config: KasIncludeGraph,
    build_dir: PathBuf,
    tasks: HashMap<String, BitBakeTask>,
}

impl TaskGraphBuilder {
    /// Create a new task graph builder from a kas file
    pub async fn from_kas_file(kas_file: &Path, build_dir: PathBuf) -> Result<Self> {
        tracing::info!("Loading kas configuration from {}", kas_file.display());
        let kas_config = KasIncludeGraph::build(kas_file).await?;

        Ok(Self {
            kas_config,
            build_dir,
            tasks: HashMap::new(),
        })
    }

    /// Get the merged kas configuration
    pub fn config(&self) -> convenient_kas::KasConfig {
        self.kas_config.merge_config()
    }

    /// Fetch repositories defined in kas configuration
    pub async fn fetch_repos(&self) -> Result<()> {
        tracing::info!("Fetching repositories...");
        let config = self.config();

        for (name, repo) in &config.repos {
            if let Some(url) = &repo.url {
                let repo_dir = self.build_dir.join("repos").join(name);

                if repo_dir.exists() {
                    tracing::debug!("Repository {} already exists", name);
                } else {
                    tracing::info!("Cloning {} from {}", name, url);

                    let branch = repo.branch.as_deref().unwrap_or("master");
                    let output = Command::new("git")
                        .args(&[
                            "clone",
                            "--branch",
                            branch,
                            "--depth",
                            "1",
                            url,
                            repo_dir.to_str().unwrap(),
                        ])
                        .output()?;

                    if !output.status.success() {
                        tracing::error!(
                            "Failed to clone {}: {}",
                            name,
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Parse BitBake recipes and extract tasks
    ///
    /// This is a simplified implementation for demonstration.
    /// A real implementation would parse actual .bb files.
    pub fn parse_recipes(&mut self) -> Result<()> {
        tracing::info!("Parsing BitBake recipes...");

        let config = self.config();
        let machine = config.machine.as_deref().unwrap_or("unknown");
        let default_targets = vec![];
        let targets = config.target.as_ref().unwrap_or(&default_targets);

        // Create standard BitBake tasks for each target
        for target in targets {
            tracing::info!("Generating tasks for {}", target);
            self.generate_tasks_for_recipe(target, machine)?;
        }

        tracing::info!("Parsed {} tasks", self.tasks.len());
        Ok(())
    }

    /// Generate standard BitBake tasks for a recipe
    fn generate_tasks_for_recipe(&mut self, recipe: &str, machine: &str) -> Result<()> {
        let recipe_dir = self
            .build_dir
            .join("tmp")
            .join("work")
            .join(machine)
            .join(recipe);

        std::fs::create_dir_all(&recipe_dir)?;

        // Standard BitBake task sequence
        let task_sequence = if recipe == "busybox" {
            vec![
                (
                    "do_fetch",
                    "Fetch source code",
                    vec![],
                    vec!["busybox-1.35.0.tar.bz2".to_string()],
                ),
                (
                    "do_unpack",
                    "Unpack source archive",
                    vec!["busybox-1.35.0.tar.bz2".to_string()],
                    vec!["busybox-1.35.0/".to_string()],
                ),
                (
                    "do_patch",
                    "Apply patches",
                    vec!["busybox-1.35.0/".to_string()],
                    vec!["busybox-1.35.0/.patched".to_string()],
                ),
                (
                    "do_configure",
                    "Configure build",
                    vec!["busybox-1.35.0/.patched".to_string()],
                    vec![".config".to_string()],
                ),
                (
                    "do_compile",
                    "Compile source",
                    vec![".config".to_string()],
                    vec!["busybox".to_string()],
                ),
                (
                    "do_install",
                    "Install to staging",
                    vec!["busybox".to_string()],
                    vec!["image/bin/busybox".to_string()],
                ),
                (
                    "do_package",
                    "Create package",
                    vec!["image/bin/busybox".to_string()],
                    vec![format!("busybox_1.35.0-r0_{}.rpm", machine)],
                ),
            ]
        } else {
            // Generic task sequence for other recipes
            vec![
                ("do_fetch", "Fetch source", vec![], vec!["src.tar.gz".to_string()]),
                (
                    "do_compile",
                    "Compile",
                    vec!["src.tar.gz".to_string()],
                    vec!["output".to_string()],
                ),
                (
                    "do_install",
                    "Install",
                    vec!["output".to_string()],
                    vec!["installed".to_string()],
                ),
            ]
        };

        for (task_name, _desc, inputs, outputs) in task_sequence {
            let task = BitBakeTask::new(
                recipe,
                task_name,
                recipe,
                "1.35.0", // Simplified - should come from recipe
                inputs,
                outputs,
                recipe_dir.clone(),
                format!("# BitBake task: {}", task_name),
            );

            self.tasks.insert(task.qualified_name(), task);
        }

        Ok(())
    }

    /// Build the task dependency graph
    pub fn build_graph(&self) -> Result<DAG<BitBakeTask, ()>> {
        tracing::info!("Building task dependency graph...");

        let mut dag = DAG::<BitBakeTask, ()>::new();
        let mut task_nodes = HashMap::new();

        // Add all tasks as nodes
        for (name, task) in &self.tasks {
            let node_id = dag.add_node(task.clone());
            task_nodes.insert(name.clone(), node_id);
        }

        // Create edges based on task dependencies
        // For each recipe, connect tasks in sequence
        let config = self.config();
        let default_targets = vec![];
        let targets = config.target.as_ref().unwrap_or(&default_targets);

        for target in targets {
            if target == "busybox" {
                let task_order = vec![
                    format!("{}:do_fetch", target),
                    format!("{}:do_unpack", target),
                    format!("{}:do_patch", target),
                    format!("{}:do_configure", target),
                    format!("{}:do_compile", target),
                    format!("{}:do_install", target),
                    format!("{}:do_package", target),
                ];

                for window in task_order.windows(2) {
                    if let (Some(&from), Some(&to)) = (task_nodes.get(&window[0]), task_nodes.get(&window[1])) {
                        dag.add_edge(from, to, ())?;
                    }
                }
            }
        }

        tracing::info!(
            "Graph built: {} nodes, {} edges",
            dag.node_count(),
            dag.edge_count()
        );

        Ok(dag)
    }

    /// Get all parsed tasks
    pub fn tasks(&self) -> &HashMap<String, BitBakeTask> {
        &self.tasks
    }
}
