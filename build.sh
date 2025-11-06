#!/bin/bash
set -e

echo "Building CSI Certificate Driver..."

# Build directory
cd src

# Build release binaries
echo "Building Rust binaries..."
cargo build --release

echo "Build complete!"
echo ""
echo "Binaries location:"
echo "  - CSI Driver: target/release/csi-driver"
echo "  - Cert Service: target/release/cacsi-service"
echo ""
echo "To build Docker image, run:"
echo "  docker build -t cacsi-driver:latest ."
