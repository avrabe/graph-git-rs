//! Build report generation (JSON, HTML, Markdown)

use serde::{Serialize, Deserialize};

/// Build report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildReport {
    pub status: BuildStatus,
    pub start_time: String,
    pub duration_s: f64,
    pub tasks: Vec<TaskReport>,
    pub cache_stats: CacheStats,
    pub resource_usage: ResourceUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuildStatus {
    Success,
    Failed,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReport {
    pub name: String,
    pub status: String,
    pub duration_ms: u64,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub hit_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub peak_memory_mb: u64,
    pub total_cpu_s: f64,
    pub io_read_mb: u64,
    pub io_write_mb: u64,
}

impl BuildReport {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn to_html(&self) -> String {
        format!(r#"
<!DOCTYPE html>
<html>
<head>
    <title>Build Report</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 20px; }}
        h1 {{ color: #333; }}
        .stats {{ background: #f4f4f4; padding: 15px; border-radius: 5px; }}
        .success {{ color: green; }}
        .failed {{ color: red; }}
        table {{ border-collapse: collapse; width: 100%; margin-top: 20px; }}
        th, td {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
        th {{ background-color: #4CAF50; color: white; }}
    </style>
</head>
<body>
    <h1>Build Report - {:?}</h1>
    <div class="stats">
        <p>Duration: {:.2}s</p>
        <p>Cache Hit Rate: {:.1}%</p>
        <p>Peak Memory: {} MB</p>
    </div>
    <h2>Tasks</h2>
    <table>
        <tr><th>Task</th><th>Status</th><th>Duration</th><th>Cached</th></tr>
        {}
    </table>
</body>
</html>"#,
            self.status,
            self.duration_s,
            self.cache_stats.hit_rate,
            self.resource_usage.peak_memory_mb,
            self.tasks.iter()
                .map(|t| format!("<tr><td>{}</td><td>{}</td><td>{}ms</td><td>{}</td></tr>",
                    t.name, t.status, t.duration_ms, if t.cached { "✓" } else { "" }))
                .collect::<Vec<_>>()
                .join("\n        ")
        )
    }

    pub fn to_markdown(&self) -> String {
        format!(r#"# Build Report

## Status: {:?}

- **Duration**: {:.2}s
- **Cache Hit Rate**: {:.1}%
- **Peak Memory**: {} MB

## Tasks

| Task | Status | Duration | Cached |
|------|--------|----------|--------|
{}

## Resource Usage

- CPU Time: {:.2}s
- I/O Read: {} MB
- I/O Write: {} MB
"#,
            self.status,
            self.duration_s,
            self.cache_stats.hit_rate,
            self.resource_usage.peak_memory_mb,
            self.tasks.iter()
                .map(|t| format!("| {} | {} | {}ms | {} |",
                    t.name, t.status, t.duration_ms, if t.cached { "✓" } else { "" }))
                .collect::<Vec<_>>()
                .join("\n"),
            self.resource_usage.total_cpu_s,
            self.resource_usage.io_read_mb,
            self.resource_usage.io_write_mb,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_generation() {
        let report = BuildReport {
            status: BuildStatus::Success,
            start_time: "2025-01-01T00:00:00Z".to_string(),
            duration_s: 123.45,
            tasks: vec![],
            cache_stats: CacheStats { hits: 10, misses: 5, hit_rate: 66.7 },
            resource_usage: ResourceUsage {
                peak_memory_mb: 512,
                total_cpu_s: 100.0,
                io_read_mb: 1024,
                io_write_mb: 512,
            },
        };

        let json = report.to_json().unwrap();
        assert!(json.contains("Success"));
        assert!(json.contains("123.45"));
    }

    #[test]
    fn test_html_generation() {
        let report = BuildReport {
            status: BuildStatus::Success,
            start_time: "2025-01-01T00:00:00Z".to_string(),
            duration_s: 10.5,
            tasks: vec![],
            cache_stats: CacheStats { hits: 5, misses: 1, hit_rate: 83.3 },
            resource_usage: ResourceUsage {
                peak_memory_mb: 256,
                total_cpu_s: 50.0,
                io_read_mb: 512,
                io_write_mb: 256,
            },
        };

        let html = report.to_html();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("10.50s"));
        assert!(html.contains("83.3%"));
    }
}
