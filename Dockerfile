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
RUN cargo build --release --bin loom_exex && \
    cargo build --release -p nodebench && \
    cargo build --release -p gasbench && \
    cargo build --release -p exex_grpc_node && \
    cargo build --release -p exex_grpc_loom && \
    cargo build --release -p loom_anvil && \
    cargo build --release -p loom_backrun && \
    cargo build --release -p loom_base && \
    cargo build --release -p nodebench && \
    cargo build --release -p replayer

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

# Copy the binaries from the builder stage
COPY --from=builder /app/target/release/loom_exex /app/
COPY --from=builder /app/target/release/nodebench /app/
COPY --from=builder /app/target/release/gasbench /app/
COPY --from=builder /app/target/release/exex_grpc_node /app/
COPY --from=builder /app/target/release/exex_grpc_loom /app/
COPY --from=builder /app/target/release/loom_anvil /app/
COPY --from=builder /app/target/release/loom_backrun /app/
COPY --from=builder /app/target/release/loom_base /app/
COPY --from=builder /app/target/release/nodebench /app/
COPY --from=builder /app/target/release/replayer /app/

# Copy configuration files from builder stage
COPY --from=builder /app/config-example.toml /app/config-example.toml
COPY --from=builder /app/config_base.toml /app/config_base.toml

# Create empty config files if they don't exist
RUN if [ ! -f /app/config-example.toml ]; then touch /app/config-example.toml; fi && \
    if [ ! -f /app/config_base.toml ]; then touch /app/config_base.toml; fi && \
    if [ ! -f /app/config.toml ]; then \
      if [ -f /app/config-example.toml ] && [ -s /app/config-example.toml ]; then \
        cp /app/config-example.toml /app/config.toml; \
      elif [ -f /app/config_base.toml ] && [ -s /app/config_base.toml ]; then \
        cp /app/config_base.toml /app/config.toml; \
      else \
        touch /app/config.toml; \
      fi; \
    fi

# Set ownership of the application directory
RUN chown -R loom:loom /app

# Switch to the non-root user
USER loom

# Set the entrypoint
# Use shell form to pass all arguments correctly
ENTRYPOINT ["/bin/sh", "-c"]
CMD /app/loom_exex remote --engine.persistence-threshold 2 --engine.memory-block-buffer-target 2

# Health check
HEALTHCHECK --interval=30s --timeout=30s --start-period=5s --retries=3 \
    CMD ps aux | grep loom_exex | grep -v grep || exit 1
