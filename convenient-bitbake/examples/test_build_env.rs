//! Test BuildEnvironment with real Poky configuration
//!
//! This example demonstrates loading a real BitBake build environment
//! from a build directory with bblayers.conf and local.conf.

use convenient_bitbake::BuildEnvironment;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║     Test BuildEnvironment with Real Poky Config       ║");
    println!("╚════════════════════════════════════════════════════════╝\n");

    // Load build environment
    let build_dir = "/home/user/graph-git-rs/poky-test/build";
    println!("Loading build environment from: {}", build_dir);

    let env = BuildEnvironment::from_build_dir(build_dir)?;

    println!("\n📋 Build Environment:");
    println!("  TOPDIR:      {:?}", env.topdir);
    println!("  OEROOT:      {:?}", env.oeroot);
    println!("  DL_DIR:      {:?}", env.dl_dir);
    println!("  TMPDIR:      {:?}", env.tmpdir);
    println!("  SSTATE_DIR:  {:?}", env.sstate_dir);
    println!("  MACHINE:     {:?}", env.get_machine());
    println!("  DISTRO:      {:?}", env.get_distro());

    println!("\n🏗️  Layers ({}):", env.layers.len());
    for (i, layer) in env.layers.iter().enumerate() {
        println!("  {}. {:?}", i + 1, layer);
    }

    println!("\n🔧 Creating BuildContext...");
    let build_context = env.create_build_context()?;

    println!("\n✅ BuildContext created successfully!");
    println!("  Layers:  {}", build_context.layers.len());
    println!("  Machine: {:?}", build_context.machine);
    println!("  Distro:  {:?}", build_context.distro);

    println!("\n📊 Layer Details:");
    for layer in &build_context.layers {
        println!(
            "  • {} (priority: {}, version: {:?})",
            layer.collection, layer.priority, layer.version
        );
        if !layer.depends.is_empty() {
            println!("    Depends: {}", layer.depends.join(", "));
        }
    }

    println!("\n🎉 Success! BuildEnvironment works with real Poky!\n");

    Ok(())
}
