fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use environment variable if protoc is installed, otherwise provide helpful message
    if std::env::var("PROTOC").is_err() {
        eprintln!("PROTOC environment variable not set.");
        eprintln!("Please install protoc or set PROTOC to the path of the protoc binary.");
        eprintln!("");
        eprintln!("Installation options:");
        eprintln!("  - Windows: Download from https://github.com/protocolbuffers/protobuf/releases");
        eprintln!("            Extract and add to PATH, or set PROTOC=path\\to\\protoc.exe");
        eprintln!("  - Linux: apt-get install protobuf-compiler");
        eprintln!("  - macOS: brew install protobuf");
        eprintln!("");
        eprintln!("For Docker builds, this is handled by the Dockerfile.");
        return Err("protoc not found".into());
    }

    // Compile CSI protobuf definitions
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(
            &["proto/csi.proto"],
            &["proto/"],
        )?;

    // Compile certificate service protobuf definitions
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(
            &["proto/cert_service.proto"],
            &["proto/"],
        )?;

    Ok(())
}
