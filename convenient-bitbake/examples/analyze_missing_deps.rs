// Analyze recipes that don't have DEPENDS extracted
// This helps identify what patterns we're missing

use convenient_bitbake::{RecipeExtractor, RecipeGraph, ExtractionConfig};
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("=== Analyzing Recipes Without DEPENDS ===\n");

    let poky_meta = PathBuf::from("/home/user/graph-git-rs/poky/meta");

    if !poky_meta.exists() {
        eprintln!("Error: poky/meta not found!");
        return;
    }

    // Find all recipes
    let recipes = find_recipes(&poky_meta);

    let mut config = ExtractionConfig::default();
    config.use_simple_python_eval = true;
    config.extract_tasks = true;
    config.resolve_providers = true;
    config.resolve_includes = true;
    config.resolve_inherit = true;
    config.extract_class_deps = true;
    config.class_search_paths = vec![poky_meta.clone()];

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    let mut with_depends = Vec::new();
    let mut without_depends = Vec::new();

    for recipe_path in recipes {
        let content = match fs::read_to_string(&recipe_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let recipe_name = recipe_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        match extractor.extract_from_content(&mut graph, recipe_name, &content) {
            Ok(extraction) => {
                if extraction.depends.is_empty() {
                    without_depends.push((recipe_name.to_string(), recipe_path.clone(), content.clone()));
                } else {
                    with_depends.push(recipe_name.to_string());
                }
            }
            Err(_) => {}
        }
    }

    println!("Recipes WITH DEPENDS:    {}", with_depends.len());
    println!("Recipes WITHOUT DEPENDS: {}\n", without_depends.len());

    if !without_depends.is_empty() {
        println!("=== Recipes Missing DEPENDS ===\n");

        for (name, path, content) in &without_depends {
            println!("Recipe: {}", name);
            println!("Path: {}", path.display());

            // Analyze why DEPENDS might be missing
            let has_depends_line = content.lines().any(|l| l.trim().starts_with("DEPENDS"));
            let has_inherit = content.lines().any(|l| l.trim().starts_with("inherit"));
            let is_packagegroup = content.contains("packagegroup");
            let is_image = content.contains("inherit") && (content.contains("image") || content.contains("core-image"));

            println!("  Has DEPENDS line:     {}", has_depends_line);
            println!("  Has inherit:          {}", has_inherit);
            println!("  Is packagegroup:      {}", is_packagegroup);
            println!("  Is image:             {}", is_image);

            // Show first 40 lines to understand structure
            println!("  First 40 lines:");
            for (i, line) in content.lines().take(40).enumerate() {
                if line.trim().starts_with("DEPENDS") || line.trim().starts_with("inherit") {
                    println!("    {:3}: {}", i+1, line);
                }
            }

            println!();
        }
    }

    println!("\n=== Summary ===");
    println!("Accuracy: {:.1}%", (with_depends.len() as f64 / (with_depends.len() + without_depends.len()) as f64) * 100.0);
}

fn find_recipes(base_path: &PathBuf) -> Vec<PathBuf> {
    let mut recipes = Vec::new();

    let recipes_core = base_path.join("recipes-core");
    if recipes_core.exists() {
        find_bb_files(&recipes_core, &mut recipes);
    }

    recipes
}

fn find_bb_files(dir: &PathBuf, results: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                find_bb_files(&path, results);
            } else if path.extension().and_then(|s| s.to_str()) == Some("bb") {
                // Skip .bbappend files
                if !path.to_str().unwrap_or("").contains(".bbappend") {
                    results.push(path);
                }
            }
        }
    }
}
