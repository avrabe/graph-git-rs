// BitBake Recipe Dependency Extraction Accuracy Measurement Tool
// Tests Phase 10 Python IR improvements

use clap::{Parser, Subcommand};
use colored::Colorize;
use convenient_bitbake::{ExtractionConfig, RecipeExtractor, RecipeGraph};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "measure-accuracy")]
#[command(about = "Measure BitBake dependency extraction accuracy", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan a directory for .bb files and measure extraction accuracy
    Scan {
        /// Directory containing BitBake recipes
        #[arg(short, long)]
        dir: PathBuf,

        /// Output directory for results
        #[arg(short, long, default_value = "accuracy-results")]
        output: PathBuf,

        /// Compare with/without Phase 10
        #[arg(long)]
        compare: bool,
    },

    /// Test on kas-configured projects
    Kas {
        /// Path to kas configuration file
        #[arg(short, long)]
        config: PathBuf,

        /// Output directory for results
        #[arg(short, long, default_value = "accuracy-results")]
        output: PathBuf,
    },

    /// Analyze results and generate report
    Report {
        /// Results directory
        #[arg(short, long, default_value = "accuracy-results")]
        input: PathBuf,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct RecipeMeasurement {
    name: String,
    file_path: String,
    has_python_blocks: bool,
    python_block_count: usize,
    depends_without_phase10: Vec<String>,
    depends_with_phase10: Vec<String>,
    rdepends_without_phase10: Vec<String>,
    rdepends_with_phase10: Vec<String>,
    phase10_added_depends: Vec<String>,
    phase10_added_rdepends: Vec<String>,
    extraction_time_ms: u128,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccuracyReport {
    total_recipes: usize,
    recipes_with_python: usize,
    phase10_impact_count: usize,
    total_deps_added: usize,
    total_rdeps_added: usize,
    measurements: Vec<RecipeMeasurement>,
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Scan { dir, output, compare } => {
            scan_directory(dir, output, *compare);
        }
        Commands::Kas { config, output } => {
            test_kas_config(config, output);
        }
        Commands::Report { input } => {
            generate_report(input);
        }
    }
}

fn scan_directory(dir: &Path, output_dir: &Path, compare: bool) {
    println!("{}", "=== BitBake Recipe Accuracy Measurement ===".bold().green());
    println!("Scanning directory: {}", dir.display());

    // Create output directory
    fs::create_dir_all(output_dir).expect("Failed to create output directory");

    // Find all .bb files
    let recipes = find_bb_files(dir);
    println!("Found {} recipe files", recipes.len());

    if compare {
        println!("\n{}", "Running comparison: With/Without Phase 10".bold().yellow());
        let report = measure_with_comparison(&recipes);
        save_report(&report, output_dir);
        print_summary(&report);
    } else {
        println!("\n{}", "Running measurement with Phase 10 enabled".bold().yellow());
        let report = measure_recipes(&recipes, true);
        save_report(&report, output_dir);
        print_summary(&report);
    }
}

fn find_bb_files(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension()
                .and_then(|s| s.to_str())
                .map(|s| s == "bb")
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn measure_recipes(recipes: &[PathBuf], phase10_enabled: bool) -> AccuracyReport {
    let pb = ProgressBar::new(recipes.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut measurements = Vec::new();
    let mut config = ExtractionConfig::default();
    config.use_python_ir = phase10_enabled;
    config.use_simple_python_eval = true;

    // Enable RustPython for complex Python patterns (Phase 11)
    #[cfg(feature = "python-execution")]
    {
        config.use_python_executor = phase10_enabled;
    }

    // Set up default variables for Python block evaluation
    config.default_variables.insert("DISTRO_FEATURES".to_string(), "systemd pam usrmerge".to_string());
    config.default_variables.insert("PACKAGECONFIG".to_string(), "feature1 feature2".to_string());
    config.default_variables.insert("MACHINE".to_string(), "qemux86-64".to_string());

    for recipe_path in recipes {
        pb.inc(1);

        if let Ok(content) = fs::read_to_string(recipe_path) {
            let recipe_name = recipe_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let has_python = content.contains("python __anonymous()");
            let python_count = content.matches("python __anonymous()").count();

            let start = std::time::Instant::now();
            let extractor = RecipeExtractor::new(config.clone());
            let mut graph = RecipeGraph::new();

            if let Ok(extraction) = extractor.extract_from_content(&mut graph, &recipe_name, &content) {
                let elapsed = start.elapsed().as_millis();

                measurements.push(RecipeMeasurement {
                    name: recipe_name,
                    file_path: recipe_path.display().to_string(),
                    has_python_blocks: has_python,
                    python_block_count: python_count,
                    depends_without_phase10: Vec::new(),
                    depends_with_phase10: extraction.depends.clone(),
                    rdepends_without_phase10: Vec::new(),
                    rdepends_with_phase10: extraction.rdepends.clone(),
                    phase10_added_depends: Vec::new(),
                    phase10_added_rdepends: Vec::new(),
                    extraction_time_ms: elapsed,
                });
            }
        }
    }

    pb.finish_with_message("Measurement complete!");

    let recipes_with_python = measurements.iter().filter(|m| m.has_python_blocks).count();

    AccuracyReport {
        total_recipes: measurements.len(),
        recipes_with_python,
        phase10_impact_count: 0,
        total_deps_added: 0,
        total_rdeps_added: 0,
        measurements,
    }
}

fn measure_with_comparison(recipes: &[PathBuf]) -> AccuracyReport {
    println!("Phase 1: Measuring WITHOUT Phase 10...");
    let without = measure_recipes(recipes, false);

    println!("Phase 2: Measuring WITH Phase 10...");
    let with = measure_recipes(recipes, true);

    // Compare results
    let mut measurements = Vec::new();
    let mut phase10_impact_count = 0;
    let mut total_deps_added = 0;
    let mut total_rdeps_added = 0;

    for (idx, with_measurement) in with.measurements.iter().enumerate() {
        if idx < without.measurements.len() {
            let without_measurement = &without.measurements[idx];

            let deps_without: HashSet<_> = without_measurement.depends_with_phase10.iter().collect();
            let deps_with: HashSet<_> = with_measurement.depends_with_phase10.iter().collect();

            let added_deps: Vec<String> = deps_with
                .difference(&deps_without)
                .map(|s| s.to_string())
                .collect();

            let rdeps_without: HashSet<_> = without_measurement.rdepends_with_phase10.iter().collect();
            let rdeps_with: HashSet<_> = with_measurement.rdepends_with_phase10.iter().collect();

            let added_rdeps: Vec<String> = rdeps_with
                .difference(&rdeps_without)
                .map(|s| s.to_string())
                .collect();

            let has_impact = !added_deps.is_empty() || !added_rdeps.is_empty();
            if has_impact {
                phase10_impact_count += 1;
                total_deps_added += added_deps.len();
                total_rdeps_added += added_rdeps.len();
            }

            measurements.push(RecipeMeasurement {
                name: with_measurement.name.clone(),
                file_path: with_measurement.file_path.clone(),
                has_python_blocks: with_measurement.has_python_blocks,
                python_block_count: with_measurement.python_block_count,
                depends_without_phase10: without_measurement.depends_with_phase10.clone(),
                depends_with_phase10: with_measurement.depends_with_phase10.clone(),
                rdepends_without_phase10: without_measurement.rdepends_with_phase10.clone(),
                rdepends_with_phase10: with_measurement.rdepends_with_phase10.clone(),
                phase10_added_depends: added_deps,
                phase10_added_rdepends: added_rdeps,
                extraction_time_ms: with_measurement.extraction_time_ms,
            });
        }
    }

    AccuracyReport {
        total_recipes: measurements.len(),
        recipes_with_python: with.recipes_with_python,
        phase10_impact_count,
        total_deps_added,
        total_rdeps_added,
        measurements,
    }
}

fn test_kas_config(config_path: &Path, output_dir: &Path) {
    println!("{}", "=== Testing Kas Configuration ===".bold().green());
    println!("Config: {}", config_path.display());

    // TODO: Implement kas integration
    println!("{}", "Note: Full kas integration requires kas to be installed".yellow());
    println!("For now, manually run:");
    println!("  kas shell {} -c 'bitbake-layers show-recipes'", config_path.display());
    println!("Then use 'scan' command on the build directory");
}

fn save_report(report: &AccuracyReport, output_dir: &Path) {
    let report_path = output_dir.join("accuracy-report.json");
    let json = serde_json::to_string_pretty(report).expect("Failed to serialize report");
    fs::write(&report_path, json).expect("Failed to write report");
    println!("\n{} {}", "Report saved to:".green(), report_path.display());
}

fn print_summary(report: &AccuracyReport) {
    println!("\n{}", "=== Summary ===".bold().cyan());
    println!("Total recipes analyzed: {}", report.total_recipes);
    println!("Recipes with Python blocks: {} ({:.1}%)",
        report.recipes_with_python,
        (report.recipes_with_python as f64 / report.total_recipes as f64) * 100.0
    );

    if report.phase10_impact_count > 0 {
        println!("\n{}", "=== Phase 10 Impact ===".bold().green());
        println!("Recipes affected by Phase 10: {} ({:.1}%)",
            report.phase10_impact_count,
            (report.phase10_impact_count as f64 / report.total_recipes as f64) * 100.0
        );
        println!("Total DEPENDS added: {}", report.total_deps_added);
        println!("Total RDEPENDS added: {}", report.total_rdeps_added);

        println!("\n{}", "Top recipes with most changes:".yellow());
        let mut sorted: Vec<_> = report.measurements.iter()
            .filter(|m| !m.phase10_added_depends.is_empty() || !m.phase10_added_rdepends.is_empty())
            .collect();
        sorted.sort_by_key(|m| std::cmp::Reverse(m.phase10_added_depends.len() + m.phase10_added_rdepends.len()));

        for (idx, m) in sorted.iter().take(10).enumerate() {
            println!("  {}. {} ({} DEPENDS, {} RDEPENDS)",
                idx + 1,
                m.name.bold(),
                m.phase10_added_depends.len(),
                m.phase10_added_rdepends.len()
            );
            if !m.phase10_added_depends.is_empty() {
                println!("     Added DEPENDS: {}", m.phase10_added_depends.join(", "));
            }
            if !m.phase10_added_rdepends.is_empty() {
                println!("     Added RDEPENDS: {}", m.phase10_added_rdepends.join(", "));
            }
        }
    }

    // Average extraction time
    let avg_time: f64 = report.measurements.iter()
        .map(|m| m.extraction_time_ms as f64)
        .sum::<f64>() / report.measurements.len() as f64;

    println!("\n{}", "=== Performance ===".bold().cyan());
    println!("Average extraction time: {:.2}ms", avg_time);
}

fn generate_report(input_dir: &Path) {
    println!("{}", "=== Generating Report ===".bold().green());

    let report_path = input_dir.join("accuracy-report.json");
    if !report_path.exists() {
        eprintln!("{}", "Error: accuracy-report.json not found".red());
        return;
    }

    let json = fs::read_to_string(&report_path).expect("Failed to read report");
    let report: AccuracyReport = serde_json::from_str(&json).expect("Failed to parse report");

    print_summary(&report);

    // Generate detailed markdown report
    let markdown = generate_markdown_report(&report);
    let md_path = input_dir.join("ACCURACY_REPORT.md");
    fs::write(&md_path, markdown).expect("Failed to write markdown report");
    println!("\n{} {}", "Detailed report saved to:".green(), md_path.display());
}

fn generate_markdown_report(report: &AccuracyReport) -> String {
    let mut md = String::new();

    md.push_str("# BitBake Dependency Extraction Accuracy Report\n\n");
    md.push_str(&format!("**Total Recipes:** {}\n", report.total_recipes));
    md.push_str(&format!("**Recipes with Python Blocks:** {} ({:.1}%)\n\n",
        report.recipes_with_python,
        (report.recipes_with_python as f64 / report.total_recipes as f64) * 100.0
    ));

    if report.phase10_impact_count > 0 {
        md.push_str("## Phase 10 Impact\n\n");
        md.push_str(&format!("- **Recipes Affected:** {} ({:.1}%)\n",
            report.phase10_impact_count,
            (report.phase10_impact_count as f64 / report.total_recipes as f64) * 100.0
        ));
        md.push_str(&format!("- **Total DEPENDS Added:** {}\n", report.total_deps_added));
        md.push_str(&format!("- **Total RDEPENDS Added:** {}\n\n", report.total_rdeps_added));

        md.push_str("### Recipes with Changes\n\n");
        md.push_str("| Recipe | Python Blocks | DEPENDS Added | RDEPENDS Added |\n");
        md.push_str("|--------|--------------|---------------|----------------|\n");

        for m in &report.measurements {
            if !m.phase10_added_depends.is_empty() || !m.phase10_added_rdepends.is_empty() {
                md.push_str(&format!("| {} | {} | {} | {} |\n",
                    m.name,
                    m.python_block_count,
                    m.phase10_added_depends.len(),
                    m.phase10_added_rdepends.len()
                ));
            }
        }

        md.push_str("\n### Detailed Changes\n\n");
        for m in &report.measurements {
            if !m.phase10_added_depends.is_empty() || !m.phase10_added_rdepends.is_empty() {
                md.push_str(&format!("#### {}\n\n", m.name));
                if !m.phase10_added_depends.is_empty() {
                    md.push_str(&format!("**Added DEPENDS:** `{}`\n\n", m.phase10_added_depends.join("`, `")));
                }
                if !m.phase10_added_rdepends.is_empty() {
                    md.push_str(&format!("**Added RDEPENDS:** `{}`\n\n", m.phase10_added_rdepends.join("`, `")));
                }
            }
        }
    }

    // Performance stats
    let avg_time: f64 = report.measurements.iter()
        .map(|m| m.extraction_time_ms as f64)
        .sum::<f64>() / report.measurements.len() as f64;

    md.push_str("## Performance\n\n");
    md.push_str(&format!("- **Average Extraction Time:** {:.2}ms\n", avg_time));
    md.push_str(&format!("- **Total Processing Time:** {:.2}s\n",
        report.measurements.iter().map(|m| m.extraction_time_ms).sum::<u128>() as f64 / 1000.0
    ));

    md
}
