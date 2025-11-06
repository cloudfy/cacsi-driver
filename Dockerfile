# Multi-stage build for CSI Certificate Driver

# Build stage
FROM rust:bookworm AS builder

WORKDIR /build

# Install nightly toolchain for edition2024 support
RUN rustup default nightly

# Install build dependencies
RUN apt-get update && \
    apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Set PROTOC environment variable
ENV PROTOC=/usr/bin/protoc

# Copy source code
COPY src/Cargo.toml ./
COPY src/build.rs ./
COPY src/*.rs ./
COPY src/proto ./proto
COPY src/csi ./csi
COPY src/cert_service ./cert_service

# Build the binaries
RUN cargo build --release --bins

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binaries from builder
COPY --from=builder /build/target/release/csi-driver /usr/local/bin/csi-driver
COPY --from=builder /build/target/release/cacsi-service /usr/local/bin/cacsi-service

# Create directories
RUN mkdir -p /csi /var/lib/csi-certs

# Set executable permissions
RUN chmod +x /usr/local/bin/csi-driver /usr/local/bin/cacsi-service

# Default command
CMD ["/usr/local/bin/csi-driver"]
