//! Query expression types and AST
//!
//! Represents the abstract syntax tree of query expressions.

use std::fmt;

/// A query expression (AST node)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExpr {
    /// Target pattern (e.g., "meta-core:busybox", "//...", "meta-core:...")
    Target(TargetPattern),

    /// deps(expr, max_depth) - All dependencies
    Deps {
        expr: Box<QueryExpr>,
        max_depth: Option<usize>,
    },

    /// rdeps(universe, target) - Reverse dependencies
    ReverseDeps {
        universe: Box<QueryExpr>,
        target: Box<QueryExpr>,
    },

    /// somepath(from, to) - Find a dependency path
    SomePath {
        from: Box<QueryExpr>,
        to: Box<QueryExpr>,
    },

    /// allpaths(from, to) - Find all dependency paths
    AllPaths {
        from: Box<QueryExpr>,
        to: Box<QueryExpr>,
    },

    /// kind(pattern, expr) - Filter by target type
    Kind {
        pattern: String,
        expr: Box<QueryExpr>,
    },

    /// filter(pattern, expr) - Filter by label pattern
    Filter {
        pattern: String,
        expr: Box<QueryExpr>,
    },

    /// attr(name, value, expr) - Filter by attribute
    Attr {
        name: String,
        value: String,
        expr: Box<QueryExpr>,
    },

    /// Set operations
    Intersect(Box<QueryExpr>, Box<QueryExpr>),
    Union(Box<QueryExpr>, Box<QueryExpr>),
    Except(Box<QueryExpr>, Box<QueryExpr>),

    /// Task-specific queries (for tquery)
    /// Show task script content
    Script(Box<QueryExpr>),

    /// Show task inputs
    Inputs(Box<QueryExpr>),

    /// Show task outputs
    Outputs(Box<QueryExpr>),

    /// Show task environment variables
    Env(Box<QueryExpr>),

    /// Critical path analysis
    CriticalPath(Box<QueryExpr>),
}

impl fmt::Display for QueryExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryExpr::Target(pattern) => write!(f, "{}", pattern),
            QueryExpr::Deps { expr, max_depth } => {
                if let Some(depth) = max_depth {
                    write!(f, "deps({}, {})", expr, depth)
                } else {
                    write!(f, "deps({})", expr)
                }
            }
            QueryExpr::ReverseDeps { universe, target } => {
                write!(f, "rdeps({}, {})", universe, target)
            }
            QueryExpr::SomePath { from, to } => {
                write!(f, "somepath({}, {})", from, to)
            }
            QueryExpr::AllPaths { from, to } => {
                write!(f, "allpaths({}, {})", from, to)
            }
            QueryExpr::Kind { pattern, expr } => {
                write!(f, "kind('{}', {})", pattern, expr)
            }
            QueryExpr::Filter { pattern, expr } => {
                write!(f, "filter('{}', {})", pattern, expr)
            }
            QueryExpr::Attr { name, value, expr } => {
                write!(f, "attr('{}', '{}', {})", name, value, expr)
            }
            QueryExpr::Intersect(a, b) => write!(f, "{} intersect {}", a, b),
            QueryExpr::Union(a, b) => write!(f, "{} union {}", a, b),
            QueryExpr::Except(a, b) => write!(f, "{} except {}", a, b),
            QueryExpr::Script(expr) => write!(f, "script({})", expr),
            QueryExpr::Inputs(expr) => write!(f, "inputs({})", expr),
            QueryExpr::Outputs(expr) => write!(f, "outputs({})", expr),
            QueryExpr::Env(expr) => write!(f, "env({})", expr),
            QueryExpr::CriticalPath(expr) => write!(f, "critical-path({})", expr),
        }
    }
}

/// Target pattern for matching recipes/tasks
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetPattern {
    /// All targets in all layers (//...)
    All,

    /// All targets in specific layer (meta-core:...)
    LayerAll(String),

    /// Specific recipe (meta-core:busybox)
    Recipe { layer: String, recipe: String },

    /// Specific task (meta-core:busybox:do_compile)
    Task {
        layer: String,
        recipe: String,
        task: String,
    },

    /// All tasks for a recipe (meta-core:busybox:*)
    RecipeAllTasks { layer: String, recipe: String },

    /// Wildcard patterns (for cross-layer searches)
    /// Recipe in any layer (*:busybox)
    WildcardRecipe { recipe: String },

    /// Task in any layer (*:busybox:configure)
    WildcardTask { recipe: String, task: String },

    /// All tasks for recipe in any layer (*:busybox:*)
    WildcardRecipeAllTasks { recipe: String },
}

impl TargetPattern {
    /// Check if this pattern matches a recipe
    pub fn matches_recipe(&self, layer: &str, recipe: &str) -> bool {
        match self {
            TargetPattern::All => true,
            TargetPattern::LayerAll(l) => l == layer,
            TargetPattern::Recipe {
                layer: l,
                recipe: r,
            } => l == layer && r == recipe,
            TargetPattern::Task {
                layer: l,
                recipe: r,
                ..
            } => l == layer && r == recipe,
            TargetPattern::RecipeAllTasks {
                layer: l,
                recipe: r,
            } => l == layer && r == recipe,
            TargetPattern::WildcardRecipe { recipe: r } => r == recipe,
            TargetPattern::WildcardTask { recipe: r, .. } => r == recipe,
            TargetPattern::WildcardRecipeAllTasks { recipe: r } => r == recipe,
        }
    }

    /// Check if this pattern matches a task
    pub fn matches_task(&self, layer: &str, recipe: &str, task: &str) -> bool {
        match self {
            TargetPattern::All => true,
            TargetPattern::LayerAll(l) => l == layer,
            TargetPattern::Recipe {
                layer: l,
                recipe: r,
            } => l == layer && r == recipe,
            TargetPattern::Task {
                layer: l,
                recipe: r,
                task: t,
            } => l == layer && r == recipe && t == task,
            TargetPattern::RecipeAllTasks {
                layer: l,
                recipe: r,
            } => l == layer && r == recipe,
            TargetPattern::WildcardRecipe { recipe: r } => r == recipe,
            TargetPattern::WildcardTask { recipe: r, task: t } => r == recipe && t == task,
            TargetPattern::WildcardRecipeAllTasks { recipe: r } => r == recipe,
        }
    }
}

impl fmt::Display for TargetPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetPattern::All => write!(f, "//..."),
            TargetPattern::LayerAll(layer) => write!(f, "{}:...", layer),
            TargetPattern::Recipe { layer, recipe } => write!(f, "{}:{}", layer, recipe),
            TargetPattern::Task {
                layer,
                recipe,
                task,
            } => write!(f, "{}:{}:{}", layer, recipe, task),
            TargetPattern::RecipeAllTasks { layer, recipe } => {
                write!(f, "{}:{}:*", layer, recipe)
            }
            TargetPattern::WildcardRecipe { recipe } => write!(f, "*:{}", recipe),
            TargetPattern::WildcardTask { recipe, task } => write!(f, "*:{}:{}", recipe, task),
            TargetPattern::WildcardRecipeAllTasks { recipe } => write!(f, "*:{}:*", recipe),
        }
    }
}

impl std::str::FromStr for TargetPattern {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "//..." {
            return Ok(TargetPattern::All);
        }

        let parts: Vec<&str> = s.split(':').collect();

        match parts.len() {
            1 => Err(format!("Invalid target pattern: {}", s)),
            2 => {
                let layer = parts[0];
                let recipe_or_wildcard = parts[1];

                // Handle wildcard layer
                if layer == "*" {
                    Ok(TargetPattern::WildcardRecipe {
                        recipe: recipe_or_wildcard.to_string(),
                    })
                } else if recipe_or_wildcard == "..." {
                    Ok(TargetPattern::LayerAll(layer.to_string()))
                } else {
                    Ok(TargetPattern::Recipe {
                        layer: layer.to_string(),
                        recipe: recipe_or_wildcard.to_string(),
                    })
                }
            }
            3 => {
                let layer = parts[0];
                let recipe = parts[1].to_string();
                let task = parts[2];

                // Handle wildcard layer
                if layer == "*" {
                    if task == "*" {
                        Ok(TargetPattern::WildcardRecipeAllTasks { recipe })
                    } else {
                        Ok(TargetPattern::WildcardTask {
                            recipe,
                            task: task.to_string(),
                        })
                    }
                } else if task == "*" {
                    Ok(TargetPattern::RecipeAllTasks {
                        layer: layer.to_string(),
                        recipe,
                    })
                } else {
                    Ok(TargetPattern::Task {
                        layer: layer.to_string(),
                        recipe,
                        task: task.to_string(),
                    })
                }
            }
            _ => Err(format!("Invalid target pattern: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_target_pattern_parsing() {
        assert_eq!(
            TargetPattern::from_str("//...").unwrap(),
            TargetPattern::All
        );

        assert_eq!(
            TargetPattern::from_str("meta-core:...").unwrap(),
            TargetPattern::LayerAll("meta-core".to_string())
        );

        assert_eq!(
            TargetPattern::from_str("meta-core:busybox").unwrap(),
            TargetPattern::Recipe {
                layer: "meta-core".to_string(),
                recipe: "busybox".to_string()
            }
        );

        assert_eq!(
            TargetPattern::from_str("meta-core:busybox:do_compile").unwrap(),
            TargetPattern::Task {
                layer: "meta-core".to_string(),
                recipe: "busybox".to_string(),
                task: "do_compile".to_string()
            }
        );

        assert_eq!(
            TargetPattern::from_str("meta-core:busybox:*").unwrap(),
            TargetPattern::RecipeAllTasks {
                layer: "meta-core".to_string(),
                recipe: "busybox".to_string()
            }
        );
    }

    #[test]
    fn test_pattern_matching() {
        let all = TargetPattern::All;
        assert!(all.matches_recipe("meta-core", "busybox"));
        assert!(all.matches_task("meta-core", "busybox", "do_compile"));

        let layer_all = TargetPattern::LayerAll("meta-core".to_string());
        assert!(layer_all.matches_recipe("meta-core", "busybox"));
        assert!(!layer_all.matches_recipe("meta-custom", "busybox"));

        let recipe = TargetPattern::Recipe {
            layer: "meta-core".to_string(),
            recipe: "busybox".to_string(),
        };
        assert!(recipe.matches_recipe("meta-core", "busybox"));
        assert!(!recipe.matches_recipe("meta-core", "glibc"));
        assert!(recipe.matches_task("meta-core", "busybox", "do_compile"));

        let task = TargetPattern::Task {
            layer: "meta-core".to_string(),
            recipe: "busybox".to_string(),
            task: "do_compile".to_string(),
        };
        assert!(task.matches_task("meta-core", "busybox", "do_compile"));
        assert!(!task.matches_task("meta-core", "busybox", "do_install"));
    }

    #[test]
    fn test_display() {
        assert_eq!(TargetPattern::All.to_string(), "//...");
        assert_eq!(
            TargetPattern::LayerAll("meta-core".to_string()).to_string(),
            "meta-core:..."
        );
        assert_eq!(
            TargetPattern::Recipe {
                layer: "meta-core".to_string(),
                recipe: "busybox".to_string()
            }
            .to_string(),
            "meta-core:busybox"
        );
    }
}
