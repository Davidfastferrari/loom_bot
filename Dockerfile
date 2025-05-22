FROM rust:1.84-slim-bullseye as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    git \
    llvm \
    libclang-dev \
    clang \
    curl \
    protobuf-compiler \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Create a new empty project
WORKDIR /app

# Copy over manifests and source code
COPY . .

# Build the application in release mode
RUN cargo build --release --bin loom_exex

# Runtime stage
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl1.1 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user to run the application
RUN useradd -m loom

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/loom_exex /app/loom_exex
# Copy configuration files
# COPY --from=builder /app/config.toml /app/config.toml

# Set ownership of the application directory
RUN chown -R loom:loom /app

# Switch to the non-root user
USER loom

# Set the entrypoint
ENTRYPOINT ["/app/loom_exex"]
CMD ["node"]

# Health check
HEALTHCHECK --interval=30s --timeout=30s --start-period=5s --retries=3 \
    CMD ps aux | grep loom_exex | grep -v grep || exit 1
