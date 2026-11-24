# Build stage
FROM rust:1.89-bookworm AS builder

WORKDIR /app

# Copy manifests and source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches

# Build release binary with locked dependencies
RUN cargo build --release --locked

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies (ca-certificates for HTTPS model downloads, curl for healthcheck)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 contextd \
    && useradd --uid 10001 --gid contextd --create-home --shell /usr/sbin/nologin contextd

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/contextd /usr/local/bin/contextd

# Create directories owned by non-root user
RUN mkdir -p /app/models /app/data /workspace \
    && chown -R contextd:contextd /app /workspace

# Environment variables wired directly into contextd Config
ENV CONTEXTD_HOST=0.0.0.0
ENV CONTEXTD_PORT=3030
ENV CONTEXTD_DB_PATH=/app/data/contextd.db
ENV CONTEXTD_MODEL_PATH=/app/models
ENV CONTEXTD_WATCH_PATHS=/workspace

USER contextd

EXPOSE 3030

VOLUME ["/workspace", "/app/data", "/app/models"]

HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD curl -fsS http://localhost:3030/health || exit 1

ENTRYPOINT ["contextd"]
CMD ["daemon"]
