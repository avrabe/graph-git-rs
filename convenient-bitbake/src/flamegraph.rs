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
        // Generate SVG flame graph
        let width = 1200;
        let height = 600;
        let cell_height = 20;

        let mut svg = String::new();
        svg.push_str(&format!(
            r#"<svg width="{}" height="{}" xmlns="http://www.w3.org/2000/svg">
<style>
    .frame {{ stroke: white; stroke-width: 1; }}
    text {{ font-family: monospace; font-size: 12px; fill: black; }}
</style>
"#,
            width, height
        ));

        // Add title
        svg.push_str(&format!(
            r#"<text x="10" y="20" font-size="16" font-weight="bold">Build Flamegraph</text>"#
        ));

        // Calculate total time
        let total_time: u64 = self.times.values().sum();

        if total_time == 0 {
            svg.push_str(r#"<text x="10" y="50">No timing data available</text>"#);
            svg.push_str("</svg>");
            return svg;
        }

        // Sort items by time (descending)
        let mut items: Vec<_> = self.times.iter().collect();
        items.sort_by(|a, b| b.1.cmp(a.1));

        // Draw bars
        let mut y = 40;
        for (name, time) in items.iter().take(25) {  // Limit to top 25 items
            let bar_width = ((**time as f64 / total_time as f64) * (width as f64 - 300.0)) as usize;
            let color = self.get_color_for_time(**time, total_time);

            svg.push_str(&format!(
                r#"<rect class="frame" x="150" y="{}" width="{}" height="{}" fill="{}" />
<text x="10" y="{}" text-anchor="start">{}</text>
<text x="{}" y="{}" text-anchor="start">{} ms</text>
"#,
                y,
                bar_width,
                cell_height - 2,
                color,
                y + cell_height - 6,
                Self::truncate_name(name, 20),
                160 + bar_width,
                y + cell_height - 6,
                time
            ));

            y += cell_height;
        }

        svg.push_str("</svg>");
        svg
    }

    fn get_color_for_time(&self, time: u64, max_time: u64) -> String {
        // Color gradient from green (fast) to red (slow)
        let ratio = time as f64 / max_time as f64;
        let r = (255.0 * ratio) as u8;
        let g = (255.0 * (1.0 - ratio)) as u8;
        format!("rgb({},{},100)", r, g)
    }

    fn truncate_name(name: &str, max_len: usize) -> String {
        if name.len() <= max_len {
            name.to_string()
        } else {
            format!("{}...", &name[..max_len - 3])
        }
    }
}

impl Default for FlameGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}
