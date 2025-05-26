FROM rust:1.84-slim-bullseye as builder

# Install build dependencies
RUN apt-get update && apt-get install -y --fix-missing \
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

# Set working directory
WORKDIR /app

# Copy entire source code first
COPY . .

# Cache dependencies
RUN cargo fetch

# Build all required binaries in one command with increased jobs
RUN cargo build --release --all --bins --jobs $(nproc)

# Second stage for runtime image
FROM rust:1.84-slim-bullseye

# Create non-root user
RUN groupadd -r loom && useradd -r -g loom loom

# Set working directory
WORKDIR /app

# Copy binaries from builder stage
COPY --from=builder /app/target/release/gasbench /app/
COPY --from=builder /app/target/release/exex-grpc-node /app/
COPY --from=builder /app/target/release/exex_grpc_loom /app/
COPY --from=builder /app/target/release/loom_anvil /app/
COPY --from=builder /app/target/release/loom_backrun /app/
COPY --from=builder /app/target/release/loom_base /app/
COPY --from=builder /app/target/release/loom_exex /app/
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

# Remove redundant cleanup in runtime stage
