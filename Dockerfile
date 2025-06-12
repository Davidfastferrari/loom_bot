FROM rust:1.84-slim-bullseye as builder

# Install build dependencies
# Robust apt-get install with retries and recovery for network/package issues
RUN set -e; \
    for i in 1 2 3; do \
      apt-get update && \
      apt-get install -y --fix-missing \
        pkg-config \
        libssl-dev \
        build-essential \
        git \
        llvm \
        llvm-11 \
        llvm-11-dev \
        libclang-dev \
        clang \
        curl \
        protobuf-compiler \
        libpq-dev \
        perl \
        libalgorithm-diff-xs-perl \
        libalgorithm-merge-perl \
        libfile-fcntllock-perl \
        libalgorithm-diff-perl \
      && break || sleep 5; \
    done; \
    dpkg --configure -a; \
    apt-get install -f -y; \
    rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app
#
# Copy entire source code first
COPY . .

# Cache dependencies
RUN cargo fetch

# Build all required binaries in one command with increased jobs
# Updated to build with no default features and enable with-topology feature to avoid cyclic dependency
RUN cargo build --release --all --bins --jobs $(nproc) --no-default-features --features "with-blockchain with-block-history-actor loom-core-block-history-actor"

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
# COPY --from=builder /app/target/release/loom /app/

# Copy configuration files from builder stage
COPY --from=builder /app/config_base.toml /app/config_base.toml
COPY --from=builder /app/config_base.toml /app/config_base.toml

# Create empty config files if they don't exist
RUN if [ ! -f /app/config_base.toml ]; then touch /app/config_base.toml; fi && \
    if [ ! -f /app/config_base.toml ]; then touch /app/config_base.toml; fi && \
    if [ ! -f /app/config.toml ]; then \
      if [ -f /app/config_base.toml ] && [ -s /app/config_base.toml ]; then \
        cp /app/config_base.toml /app/config.toml; \
      elif [ -f /app/config_base.toml ] && [ -s /app/config_base.toml ]; then \
        cp /app/config_base.toml /app/config.toml; \
      else \
        touch /app/config.toml; \
      fi; \
    fi

# Set ownership of the application directory
RUN chown -R loom:loom /app

# Copy startup script
COPY start_loom.sh /app/start_loom.sh
RUN chmod +x /app/start_loom.sh

# Switch to the non-root user
USER loom

# Set environment variable for info logging
ENV RUST_LOG=info

# Set the entrypoint
# Use exec form to run the startup script directly
ENTRYPOINT ["/app/start_loom.sh"]

# Remove CMD since ENTRYPOINT runs the script directly
# CMD /app/start_loom.sh

# Health check
HEALTHCHECK --interval=30s --timeout=30s --start-period=5s --retries=3 \
    CMD ps aux | grep loom | grep -v grep || exit 1
