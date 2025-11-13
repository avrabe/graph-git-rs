// Real-world validation: Parse KAS project and compare with BitBake output
//
// This example demonstrates:
// 1. Parsing actual BitBake recipes from a KAS project
// 2. Building our dependency graph
// 3. Comparing with BitBake's task-depends.dot output
//
// Usage:
//   1. Set up KAS environment: kas shell fmu-project.yml
//   2. Generate BitBake graph: bitbake -g fmu-image
//   3. Run this tool: cargo run --example kas_validation -- /path/to/build

use convenient_bitbake::{RecipeExtractor, RecipeGraph, ExtractionConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashSet;

fn main() {
    println!("=== KAS Project Validation ===\n");

    let args: Vec<String> = std::env::args().collect();

    // Check if build directory provided
    let build_dir = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        println!("Usage: {} <build-directory>", args[0]);
        println!("\nExample workflow:");
        println!("  1. Clone repos:     kas checkout ../convenient-kas/fmu-project.yml");
        println!("  2. Enter shell:     kas shell ../convenient-kas/fmu-project.yml");
        println!("  3. Generate graph:  bitbake -g fmu-image");
        println!("  4. Run validation:  cargo run --example kas_validation build");
        println!("\nRunning with sample data instead...\n");

        run_sample_validation();
        return;
    };

    println!("Build directory: {}\n", build_dir.display());

    // Parse BitBake's output files
    let task_depends = build_dir.join("task-depends.dot");
    let pn_buildlist = build_dir.join("pn-buildlist");

    if !task_depends.exists() || !pn_buildlist.exists() {
        eprintln!("Error: BitBake graph files not found!");
        eprintln!("Expected:");
        eprintln!("  - {}", task_depends.display());
        eprintln!("  - {}", pn_buildlist.display());
        eprintln!("\nRun 'bitbake -g fmu-image' first!");
        return;
    }

    // Load our graph
    println!("--- Building Our Dependency Graph ---\n");

    let mut config = ExtractionConfig::default();
    config.use_python_executor = false;
    config.extract_tasks = true;
    config.resolve_providers = true;

    let extractor = RecipeExtractor::new(config);
    let mut our_graph = RecipeGraph::new();

    // Parse recipe files from layers
    let layers = find_layers(&build_dir);
    println!("Found {} layers:", layers.len());
    for layer in &layers {
        println!("  • {}", layer.display());
    }
    println!();

    let mut recipe_count = 0;
    let mut extractions = Vec::new();

    for layer in &layers {
        let recipes = find_recipes(layer);
        println!("Processing {} recipes from {}...",
            recipes.len(),
            layer.file_name().unwrap().to_string_lossy()
        );

        for recipe_path in recipes {
            if let Ok(content) = fs::read_to_string(&recipe_path) {
                let recipe_name = recipe_path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .split('_')
                    .next()
                    .unwrap()
                    .to_string();

                match extractor.extract_from_content(&mut our_graph, &recipe_name, &content) {
                    Ok(extraction) => {
                        recipe_count += 1;
                        extractions.push(extraction);
                    }
                    Err(e) => {
                        eprintln!("  Warning: Failed to extract {}: {}", recipe_name, e);
                    }
                }
            }
        }
    }

    // Populate dependencies
    if let Err(e) = extractor.populate_dependencies(&mut our_graph, &extractions) {
        eprintln!("Error populating dependencies: {}", e);
        return;
    }

    println!("\n✓ Extracted {} recipes\n", recipe_count);

    // Parse BitBake's output
    println!("--- Parsing BitBake's Output ---\n");

    let bitbake_recipes = parse_pn_buildlist(&pn_buildlist);
    let bitbake_deps = parse_task_depends(&task_depends);

    println!("BitBake found {} recipes", bitbake_recipes.len());
    println!("BitBake found {} task dependencies\n", bitbake_deps.len());

    // Compare graphs
    println!("--- Comparison ---\n");

    let our_stats = our_graph.statistics();
    println!("Our graph:");
    println!("  Recipes: {}", our_stats.recipe_count);
    println!("  Tasks: {}", our_stats.task_count);
    println!("  Dependencies: {}", our_stats.total_dependencies);
    println!();

    println!("BitBake graph:");
    println!("  Recipes: {}", bitbake_recipes.len());
    println!("  Task dependencies: {}", bitbake_deps.len());
    println!();

    // Find missing recipes
    let our_recipes: HashSet<_> = (0..our_stats.recipe_count)
        .filter_map(|i| our_graph.get_recipe(convenient_bitbake::RecipeId(i as u32)))
        .map(|r| r.name.clone())
        .collect();

    let missing_in_ours: Vec<_> = bitbake_recipes
        .iter()
        .filter(|r| !our_recipes.contains(*r))
        .collect();

    let extra_in_ours: Vec<_> = our_recipes
        .iter()
        .filter(|r| !bitbake_recipes.contains(r.as_str()))
        .collect();

    if !missing_in_ours.is_empty() {
        println!("⚠ Recipes in BitBake but not in our graph ({}):", missing_in_ours.len());
        for recipe in missing_in_ours.iter().take(10) {
            println!("  • {}", recipe);
        }
        if missing_in_ours.len() > 10 {
            println!("  ... and {} more", missing_in_ours.len() - 10);
        }
        println!();
    }

    if !extra_in_ours.is_empty() {
        println!("⚠ Recipes in our graph but not in BitBake ({}):", extra_in_ours.len());
        for recipe in extra_in_ours.iter().take(10) {
            println!("  • {}", recipe);
        }
        if extra_in_ours.len() > 10 {
            println!("  ... and {} more", extra_in_ours.len() - 10);
        }
        println!();
    }

    // Compare dependency counts for common recipes
    println!("--- Dependency Validation (Sample) ---\n");

    for recipe_name in bitbake_recipes.iter().take(5) {
        if let Some(recipe_id) = our_graph.find_recipe(recipe_name) {
            let our_deps = our_graph.get_dependencies(recipe_id);
            let bitbake_task_count = bitbake_deps.iter()
                .filter(|(from, _)| from.starts_with(recipe_name))
                .count();

            println!("{}: {} deps (ours) vs {} task deps (BitBake)",
                recipe_name,
                our_deps.len(),
                bitbake_task_count
            );
        }
    }

    println!("\n=== Validation Complete ===");
}

fn run_sample_validation() {
    println!("=== Sample Validation (Without BitBake) ===\n");

    let mut config = ExtractionConfig::default();
    config.use_python_executor = false;
    config.extract_tasks = true;
    config.resolve_providers = true;

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    // Check if test.bb exists
    let test_recipe = PathBuf::from("/home/user/graph-git-rs/convenient-bitbake/test.bb");

    if test_recipe.exists() {
        println!("Found test.bb, parsing...\n");

        if let Ok(content) = fs::read_to_string(&test_recipe) {
            match extractor.extract_from_content(&mut graph, "test-recipe", &content) {
                Ok(extraction) => {
                    println!("✓ Successfully parsed test.bb");
                    println!("  Variables: {}", extraction.variables.len());
                    println!("  DEPENDS: {:?}", extraction.depends);
                    println!("  RDEPENDS: {:?}", extraction.rdepends);
                    println!("  PROVIDES: {:?}", extraction.provides);
                    println!("  Tasks: {}", extraction.tasks.len());

                    if !extraction.tasks.is_empty() {
                        println!("\n  Task list:");
                        for task_name in &extraction.tasks {
                            println!("    • {}", task_name);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("✗ Failed to parse: {}", e);
                }
            }
        }
    } else {
        println!("No test recipes found in repository.");
        println!("\nTo run full validation:");
        println!("  1. Set up KAS: kas checkout convenient-kas/fmu-project.yml");
        println!("  2. Build: kas build convenient-kas/fmu-project.yml");
        println!("  3. Generate graph: cd build && bitbake -g fmu-image");
        println!("  4. Run: cargo run --example kas_validation build");
    }
}

fn find_layers(build_dir: &Path) -> Vec<PathBuf> {
    let mut layers = Vec::new();

    // Common layer locations
    let search_paths = vec![
        build_dir.parent().unwrap().join("poky/meta"),
        build_dir.parent().unwrap().join("poky/meta-poky"),
        build_dir.parent().unwrap().join("meta-openembedded/meta-oe"),
        build_dir.parent().unwrap().join("meta-custom"),
    ];

    for path in search_paths {
        if path.exists() && path.join("conf/layer.conf").exists() {
            layers.push(path);
        }
    }

    layers
}

fn find_recipes(layer: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(layer)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension()
                .map(|ext| ext == "bb" || ext == "bbappend")
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn parse_pn_buildlist(path: &Path) -> HashSet<String> {
    let mut recipes = HashSet::new();

    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let recipe = line.split_whitespace().next().unwrap_or("");
            if !recipe.is_empty() {
                recipes.insert(recipe.to_string());
            }
        }
    }

    recipes
}

fn parse_task_depends(path: &Path) -> Vec<(String, String)> {
    let mut deps = Vec::new();

    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            // Parse DOT format: "recipe.task" -> "dep.task"
            if line.contains("->") {
                let parts: Vec<&str> = line.split("->").collect();
                if parts.len() == 2 {
                    let from = parts[0].trim().trim_matches('"');
                    let to = parts[1].trim().trim_matches('"').trim_end_matches(';');
                    deps.push((from.to_string(), to.to_string()));
                }
            }
        }
    }

    deps
}
