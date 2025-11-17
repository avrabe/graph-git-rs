//! Flame graph generation for build profiling

use std::collections::HashMap;
use std::time::Duration;

/// Flame graph node
#[derive(Debug, Clone)]
pub struct FlameNode {
    pub name: String,
    pub duration_ms: u64,
    pub children: Vec<FlameNode>,
}

/// Flame graph builder
pub struct FlameGraphBuilder {
    root: FlameNode,
    stack: Vec<String>,
    times: HashMap<String, u64>,
}

impl FlameGraphBuilder {
    pub fn new() -> Self {
        Self {
            root: FlameNode {
                name: "root".to_string(),
                duration_ms: 0,
                children: Vec::new(),
            },
            stack: Vec::new(),
            times: HashMap::new(),
        }
    }

    pub fn enter(&mut self, name: String) {
        self.stack.push(name);
    }

    pub fn exit(&mut self, duration: Duration) {
        if let Some(name) = self.stack.pop() {
            *self.times.entry(name).or_insert(0) += duration.as_millis() as u64;
        }
    }

    pub fn to_svg(&self) -> String {
        // TODO: Generate actual SVG flame graph
        String::from("<svg></svg>")
    }
}

impl Default for FlameGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}
