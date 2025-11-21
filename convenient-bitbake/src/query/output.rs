//! Query output formatting
//!
//! Formats query results in various output formats.

use super::recipe_query::RecipeTarget;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Output format for query results
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text (default)
    Text,
    /// JSON output
    Json,
    /// GraphViz dot format
    Graph,
    /// List of labels only
    Label,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "graph" => Ok(OutputFormat::Graph),
            "label" => Ok(OutputFormat::Label),
            _ => Err(format!("Unknown output format: {s}")),
        }
    }
}

/// Query result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub targets: Vec<RecipeTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<QueryMetadata>,
}

/// Query metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetadata {
    pub query: String,
    pub target_count: usize,
    pub execution_time_ms: Option<u64>,
}

/// Format query results
pub fn format_results(
    targets: &[RecipeTarget],
    format: OutputFormat,
    metadata: Option<QueryMetadata>,
) -> Result<String, String> {
    match format {
        OutputFormat::Text => format_text(targets, metadata),
        OutputFormat::Json => format_json(targets, metadata),
        OutputFormat::Graph => format_graph(targets),
        OutputFormat::Label => format_label(targets),
    }
}

fn format_text(targets: &[RecipeTarget], metadata: Option<QueryMetadata>) -> Result<String, String> {
    let mut output = String::new();

    if let Some(meta) = metadata {
        output.push_str(&format!("# Query: {}\n", meta.query));
        output.push_str(&format!("# Targets: {}\n", meta.target_count));
        if let Some(time) = meta.execution_time_ms {
            output.push_str(&format!("# Execution time: {time}ms\n"));
        }
        output.push('\n');
    }

    for target in targets {
        output.push_str(&format!("{target}\n"));
    }

    Ok(output)
}

fn format_json(targets: &[RecipeTarget], metadata: Option<QueryMetadata>) -> Result<String, String> {
    let result = QueryResult {
        targets: targets.to_vec(),
        metadata,
    };

    serde_json::to_string_pretty(&result).map_err(|e| format!("JSON serialization error: {e}"))
}

fn format_graph(targets: &[RecipeTarget]) -> Result<String, String> {
    // For now, just list targets as graph nodes
    // Full implementation would include edges from RecipeGraph
    let mut output = String::new();

    output.push_str("digraph dependencies {\n");
    output.push_str("  rankdir=LR;\n");
    output.push_str("  node [shape=box];\n\n");

    for target in targets {
        let label = format!("{}:{}", target.layer, target.recipe);
        output.push_str(&format!("  \"{label}\";\n"));
    }

    output.push_str("}\n");

    Ok(output)
}

fn format_label(targets: &[RecipeTarget]) -> Result<String, String> {
    let mut output = String::new();

    for target in targets {
        output.push_str(&format!("{target}\n"));
    }

    Ok(output)
}

/// Format query results with dependency edges
pub fn format_graph_with_deps(
    targets: &[RecipeTarget],
    deps: &HashMap<RecipeTarget, Vec<RecipeTarget>>,
) -> Result<String, String> {
    let mut output = String::new();

    output.push_str("digraph dependencies {\n");
    output.push_str("  rankdir=LR;\n");
    output.push_str("  node [shape=box];\n\n");

    // Add nodes
    for target in targets {
        let label = format!("{}:{}", target.layer, target.recipe);
        output.push_str(&format!("  \"{label}\";\n"));
    }

    output.push('\n');

    // Add edges
    for (from, to_list) in deps {
        let from_label = format!("{}:{}", from.layer, from.recipe);
        for to in to_list {
            let to_label = format!("{}:{}", to.layer, to.recipe);
            output.push_str(&format!("  \"{from_label}\" -> \"{to_label}\";\n"));
        }
    }

    output.push_str("}\n");

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_graph::RecipeId;

    #[test]
    fn test_format_text() {
        let targets = vec![
            RecipeTarget {
                recipe_id: RecipeId(0),
                layer: "meta-core".to_string(),
                recipe: "busybox".to_string(),
            },
            RecipeTarget {
                recipe_id: RecipeId(1),
                layer: "meta-core".to_string(),
                recipe: "glibc".to_string(),
            },
        ];

        let result = format_text(&targets, None).unwrap();
        assert!(result.contains("meta-core:busybox"));
        assert!(result.contains("meta-core:glibc"));
    }

    #[test]
    fn test_format_json() {
        let targets = vec![RecipeTarget {
            recipe_id: RecipeId(0),
            layer: "meta-core".to_string(),
            recipe: "busybox".to_string(),
        }];

        let result = format_json(&targets, None).unwrap();
        assert!(result.contains("meta-core"));
        assert!(result.contains("busybox"));
    }

    #[test]
    fn test_format_graph() {
        let targets = vec![RecipeTarget {
            recipe_id: RecipeId(0),
            layer: "meta-core".to_string(),
            recipe: "busybox".to_string(),
        }];

        let result = format_graph(&targets).unwrap();
        assert!(result.contains("digraph"));
        assert!(result.contains("meta-core:busybox"));
    }

    #[test]
    fn test_output_format_parsing() {
        use std::str::FromStr;

        assert_eq!(OutputFormat::from_str("text").unwrap(), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("json").unwrap(), OutputFormat::Json);
        assert_eq!(
            OutputFormat::from_str("graph").unwrap(),
            OutputFormat::Graph
        );
        assert_eq!(
            OutputFormat::from_str("label").unwrap(),
            OutputFormat::Label
        );
    }
}
