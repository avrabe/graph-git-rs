// Build script to generate Rust code from protobuf definitions

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile protobuf files using tonic-build
    tonic_build::configure()
        .build_server(false)  // We only need the client
        .build_client(true)
        .compile_protos(
            &["proto/remote_execution.proto"],
            &["proto"],
        )?;

    println!("cargo:rerun-if-changed=proto/remote_execution.proto");

    Ok(())
}
