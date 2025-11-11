// Real recipe validation: Extract from actual Yocto/poky recipes
//
// This validates our extraction against real-world BitBake recipes
// without requiring a full BitBake build environment

use convenient_bitbake::{RecipeExtractor, RecipeGraph, ExtractionConfig};
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("=== Real Recipe Validation ===\n");

    // Find poky meta layer
    let poky_meta = PathBuf::from("/home/user/graph-git-rs/poky/meta");

    if !poky_meta.exists() {
        eprintln!("Error: poky/meta not found!");
        eprintln!("Run: kas checkout convenient-kas/fmu-project.yml");
        return;
    }

    println!("Found Yocto/poky meta layer\n");

    // Discover all recipes
    let recipes = find_recipes(&poky_meta);
    println!("Discovered {} recipe files\n", recipes.len());

    // Configure extractor with defaults and customizations
    let config = ExtractionConfig {
        use_simple_python_eval: true, // Phase 1: Use simple Python expression evaluator (bb.utils.contains, bb.utils.filter)
        extract_tasks: true,
        resolve_providers: true,
        resolve_includes: true, // Phase 5: Resolve require/include directives
        resolve_inherit: true,   // Resolve inherit classes for task extraction
        extract_class_deps: true, // Phase 6: Extract dependencies from inherited classes
        class_search_paths: vec![poky_meta.clone()], // Phase 7b: Parse .bbclass files dynamically
        ..Default::default() // Use defaults for use_python_executor, default_variables
    };

    let extractor = RecipeExtractor::new(config);
    let mut graph = RecipeGraph::new();

    // Track statistics
    let mut success_count = 0;
    let mut fail_count = 0;
    let mut skip_count = 0;
    let mut extractions = Vec::new();
    let mut parse_errors = Vec::new();

    let mut recipes_with_depends = 0;
    let mut recipes_with_rdepends = 0;
    let mut recipes_with_provides = 0;
    let mut recipes_with_tasks = 0;

    // Sample some recipes from different categories
    let sample_recipes: Vec<_> = recipes
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 20 == 0) // Every 20th recipe
        .map(|(_, r)| r)
        .take(50)
        .collect();

    let sample_count = sample_recipes.len();
    println!("--- Sampling {} recipes for detailed analysis ---\n", sample_count);

    for recipe_path in &sample_recipes {
        let recipe_name = recipe_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .split('_')
            .next()
            .unwrap()
            .to_string();

        // Skip if too long (complex recipes)
        let Ok(metadata) = fs::metadata(recipe_path) else {
            skip_count += 1;
            continue;
        };

        if metadata.len() > 100_000 {
            println!("  ⊘ Skipping {} (>100KB)", recipe_name);
            skip_count += 1;
            continue;
        }

        // Use extract_from_file to enable include resolution
        match extractor.extract_from_file(&mut graph, recipe_path) {
            Ok(extraction) => {
                println!("  ✓ {}", recipe_name);
                println!("      Path: {}", recipe_path.display());

                if let Some(pv) = extraction.variables.get("PV") {
                    println!("      Version: {}", pv);
                }

                if !extraction.depends.is_empty() {
                    println!("      DEPENDS: {}", extraction.depends.join(", "));
                    recipes_with_depends += 1;
                }

                if !extraction.rdepends.is_empty() {
                    println!("      RDEPENDS: {}", extraction.rdepends.join(", "));
                    recipes_with_rdepends += 1;
                }

                if !extraction.provides.is_empty() {
                    println!("      PROVIDES: {}", extraction.provides.join(", "));
                    recipes_with_provides += 1;
                }

                if !extraction.tasks.is_empty() {
                    println!("      TASKS: {}", extraction.tasks.join(", "));
                    recipes_with_tasks += 1;
                }

                println!();
                success_count += 1;
                extractions.push(extraction);
            }
            Err(e) => {
                println!("  ✗ {} - {}", recipe_name, e);
                parse_errors.push((recipe_name.clone(), e));
                fail_count += 1;
            }
        }
    }

    // Populate dependencies
    println!("\n--- Populating Dependency Graph ---\n");
    match extractor.populate_dependencies(&mut graph, &extractions) {
        Ok(()) => {
            println!("✓ Successfully populated dependency graph\n");
        }
        Err(e) => {
            eprintln!("⚠ Warning during population: {}\n", e);
        }
    }

    // Graph statistics
    let stats = graph.statistics();

    println!("=== Extraction Statistics ===\n");
    println!("Recipes processed:      {}", sample_count);
    println!("  ✓ Successfully parsed: {}", success_count);
    println!("  ✗ Parse errors:       {}", fail_count);
    println!("  ⊘ Skipped:            {}", skip_count);
    println!();

    println!("Dependency Graph:");
    println!("  Recipes:             {}", stats.recipe_count);
    println!("  Tasks:               {}", stats.task_count);
    println!("  Dependencies:        {}", stats.total_dependencies);
    println!("  Virtual providers:   {}", stats.provider_count);
    println!("  Max depth:           {}", stats.max_dependency_depth);
    println!();

    println!("Recipe Features:");
    println!("  With DEPENDS:        {} ({:.1}%)",
        recipes_with_depends,
        (recipes_with_depends as f64 / success_count as f64) * 100.0
    );
    println!("  With RDEPENDS:       {} ({:.1}%)",
        recipes_with_rdepends,
        (recipes_with_rdepends as f64 / success_count as f64) * 100.0
    );
    println!("  With PROVIDES:       {} ({:.1}%)",
        recipes_with_provides,
        (recipes_with_provides as f64 / success_count as f64) * 100.0
    );
    println!("  With tasks:          {} ({:.1}%)",
        recipes_with_tasks,
        (recipes_with_tasks as f64 / success_count as f64) * 100.0
    );
    println!();

    // Test specific well-known recipes to validate include resolution
    println!("=== Well-Known Recipe Test (with include resolution) ===\n");

    let known_recipe_paths = vec![
        ("bash", "/home/user/graph-git-rs/poky/meta/recipes-extended/bash/bash_5.2.21.bb"),
        ("glibc", "/home/user/graph-git-rs/poky/meta/recipes-core/glibc/glibc_2.39.bb"),
        ("openssl", "/home/user/graph-git-rs/poky/meta/recipes-connectivity/openssl/openssl_3.2.6.bb"),
        ("zlib", "/home/user/graph-git-rs/poky/meta/recipes-core/zlib/zlib_1.3.1.bb"),
    ];

    let mut known_graph = RecipeGraph::new();
    let mut known_extractions = Vec::new();

    for (name, path_str) in &known_recipe_paths {
        let path = PathBuf::from(path_str);
        if path.exists() {
            match extractor.extract_from_file(&mut known_graph, &path) {
                Ok(extraction) => {
                    println!("✓ {}", name);
                    if !extraction.depends.is_empty() {
                        println!("  DEPENDS: {}", extraction.depends.join(", "));
                    }
                    if !extraction.rdepends.is_empty() {
                        println!("  RDEPENDS: {}", extraction.rdepends.join(", "));
                    }
                    if !extraction.provides.is_empty() {
                        println!("  PROVIDES: {}", extraction.provides.join(", "));
                    }
                    if let Some(summary) = extraction.variables.get("SUMMARY") {
                        println!("  Summary: {}", summary);
                    }
                    println!("  Variables extracted: {}", extraction.variables.len());
                    println!();
                    known_extractions.push(extraction);
                }
                Err(e) => {
                    println!("✗ {} - {}\n", name, e);
                }
            }
        } else {
            println!("⊘ {} - not found\n", name);
        }
    }

    // Error analysis
    if !parse_errors.is_empty() {
        println!("=== Parse Errors ({}) ===\n", parse_errors.len());
        for (recipe, error) in parse_errors.iter().take(5) {
            println!("  {} - {}", recipe, error);
        }
        if parse_errors.len() > 5 {
            println!("  ... and {} more", parse_errors.len() - 5);
        }
        println!();
    }

    // Build order sample
    if stats.recipe_count > 0 {
        println!("=== Build Order (First 10) ===\n");
        match graph.topological_sort() {
            Ok(sorted) => {
                for (idx, recipe_id) in sorted.iter().take(10).enumerate() {
                    let recipe = graph.get_recipe(*recipe_id).unwrap();
                    println!("  {}. {}", idx + 1, recipe.name);
                }
                if sorted.len() > 10 {
                    println!("  ... and {} more", sorted.len() - 10);
                }
            }
            Err(e) => {
                println!("  ✗ Error: {}", e);
            }
        }
        println!();
    }

    // Summary
    println!("=== Validation Summary ===\n");

    let success_rate = (success_count as f64 / sample_count as f64) * 100.0;
    println!("Success rate: {:.1}%", success_rate);

    if success_rate > 90.0 {
        println!("✓ EXCELLENT: Parsing works very well on real recipes");
    } else if success_rate > 75.0 {
        println!("✓ GOOD: Most recipes parse successfully");
    } else if success_rate > 50.0 {
        println!("⚠ FAIR: Parsing works but needs improvement");
    } else {
        println!("✗ POOR: Significant parsing issues detected");
    }

    println!();
    println!("Next steps:");
    println!("  1. Analyze parse errors to improve extraction");
    println!("  2. Run full extraction on all {} recipes", recipes.len());
    println!("  3. Compare against BitBake graph (requires bitbake -g)");
    println!("  4. Test with custom meta-fmu recipes");
}

fn find_recipes(layer: &PathBuf) -> Vec<PathBuf> {
    walkdir::WalkDir::new(layer)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension()
                .map(|ext| ext == "bb")
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}
