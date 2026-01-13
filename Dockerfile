# Build stage
FROM rust:bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/contextd /usr/local/bin/contextd

# Create directory for models and data
RUN mkdir -p /app/models /app/data

# Environment variables
ENV CONTEXTD_DB_PATH=/app/data/contextd.db
ENV CONTEXTD_MODEL_PATH=/app/models

# Expose API port
EXPOSE 3030

# Mount point for workspace
VOLUME ["/workspace", "/app/data", "/app/models"]

# Default command
ENTRYPOINT ["contextd"]
CMD ["daemon"]
