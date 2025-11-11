// Demonstration of Python code analysis in BitBake recipes
// Shows detection and analysis of Python blocks for variable operations

use convenient_bitbake::{PythonAnalyzer, PythonBlock, PythonBlockType};

fn main() {
    println!("=== BitBake Python Code Analysis Demonstration ===\n");

    let analyzer = PythonAnalyzer::new();

    // Example 1: Simple literal assignment
    println!("[1] Simple Literal Assignment");
    println!("----------------------------");
    let code1 = r#"
python() {
    d.setVar('CARGO_HOME', '/opt/cargo')
    d.setVar('CARGO_BUILD_FLAGS', '--release')
}
"#;
    println!("Python Code:\n{}", code1);

    let block1 = PythonBlock::new(PythonBlockType::Anonymous, code1.to_string());
    let ops1 = analyzer.analyze_block(&block1);

    println!("Analysis:");
    for op in &ops1 {
        if op.is_literal {
            println!("  ✓ Literal assignment: {} = {:?}", op.var_name, op.value);
        } else {
            println!("  ⚠ Computed assignment: {} (value not extractable)", op.var_name);
        }
    }
    println!();

    // Example 2: Computed values
    println!("[2] Computed Values");
    println!("-------------------");
    let code2 = r#"
python() {
    srcdir = d.getVar('S')
    workdir = d.getVar('WORKDIR')
    d.setVar('BUILD_DIR', workdir + '/build')
    d.setVar('CONFIGURED', 'yes')
}
"#;
    println!("Python Code:\n{}", code2);

    let block2 = PythonBlock::new(PythonBlockType::Anonymous, code2.to_string());
    let ops2 = analyzer.analyze_block(&block2);

    println!("Analysis:");
    let mut reads = Vec::new();
    let mut writes = Vec::new();

    for op in &ops2 {
        match op.operation {
            convenient_bitbake::PythonOpType::GetVar => {
                reads.push(&op.var_name);
            }
            convenient_bitbake::PythonOpType::SetVar => {
                if op.is_literal {
                    writes.push(format!("{} = {:?} (literal)", op.var_name, op.value));
                } else {
                    writes.push(format!("{} (computed)", op.var_name));
                }
            }
            _ => {}
        }
    }

    println!("  Variables read:");
    for var in reads {
        println!("    - {}", var);
    }
    println!("  Variables written:");
    for var in writes {
        println!("    - {}", var);
    }
    println!();

    // Example 3: Conditional dependencies
    println!("[3] Conditional Dependencies");
    println!("----------------------------");
    let code3 = r#"
python() {
    distro_features = d.getVar('DISTRO_FEATURES')
    if distro_features and 'systemd' in distro_features:
        d.appendVar('DEPENDS', ' systemd')
    if distro_features and 'x11' in distro_features:
        d.appendVar('DEPENDS', ' libx11')
}
"#;
    println!("Python Code:\n{}", code3);

    let block3 = PythonBlock::new(PythonBlockType::Anonymous, code3.to_string());
    let ops3 = analyzer.analyze_block(&block3);

    println!("Analysis:");
    println!("  Python reads: DISTRO_FEATURES");
    println!("  Python may append to: DEPENDS");
    println!("  Possible values for DEPENDS:");
    for op in &ops3 {
        if let convenient_bitbake::PythonOpType::AppendVar = op.operation {
            if let Some(value) = &op.value {
                println!("    - '{}'", value);
            }
        }
    }
    println!("  ⚠ Actual value depends on DISTRO_FEATURES (runtime)");
    println!();

    // Example 4: Combined analysis
    println!("[4] Summary Analysis");
    println!("-------------------");
    let all_blocks = vec![block1, block2, block3];
    let summary = analyzer.analyze_blocks(&all_blocks);

    println!("Variables written by Python:");
    for var in &summary.variables_written {
        if let Some(value) = summary.get_literal_value(var) {
            println!("  ✓ {} = \"{}\" (literal, extractable)", var, value);
        } else if summary.computed_assignments.contains(var) {
            println!("  ⚠ {} (computed, not extractable)", var);
        } else {
            println!("  ⚠ {} (value unknown)", var);
        }
    }
    println!();

    println!("Variables read by Python:");
    for var in &summary.variables_read {
        println!("  - {}", var);
    }
    println!();

    // Example 5: Recommendations
    println!("[5] Recommendations for graph-git-rs");
    println!("------------------------------------");
    println!("When encountering Python code:");
    println!();
    println!("✅ CAN extract:");
    println!("  - Literal assignments: d.setVar('VAR', 'literal')");
    println!("  - Variable reads: d.getVar('VAR')");
    println!("  - Literal appends: d.appendVar('DEPENDS', ' extra')");
    println!();
    println!("⚠ SHOULD track:");
    println!("  - Which variables may be modified by Python");
    println!("  - Which variables Python depends on");
    println!("  - Provide 'confidence levels' for extracted data");
    println!();
    println!("❌ CANNOT extract (requires execution):");
    println!("  - Computed values: d.setVar('VAR', expr)");
    println!("  - Conditional logic");
    println!("  - Python expressions: ${{@...}}");
    println!();
    println!("💡 RECOMMENDATION:");
    println!("  Use static analysis for 90% of recipes (no or simple Python)");
    println!("  Mark Python-dependent values with low confidence");
    println!("  For critical packages: run actual BitBake if needed");
    println!();

    // Statistics
    println!("=== Statistics from Examples ===");
    println!("Total variables written: {}", summary.variables_written.len());
    println!("  - Literal (extractable): {}", summary.literal_assignments.len());
    println!("  - Computed (not extractable): {}", summary.computed_assignments.len());
    println!("Total variables read: {}", summary.variables_read.len());
    println!();

    println!("Extraction accuracy: {:.1}%",
        (summary.literal_assignments.len() as f64 / summary.variables_written.len() as f64) * 100.0
    );
}
