FROM rust:1.98.1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/to2false/racknerd-exporter" \
      org.opencontainers.image.description="Read-only RackNerd / SolusVM client API exporter for Prometheus" \
      org.opencontainers.image.licenses="MIT"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/racknerd-exporter /usr/local/bin/racknerd-exporter
USER 65532:65532
ENV RACKNERD_LISTEN_ADDRESS=0.0.0.0:9725
EXPOSE 9725
ENTRYPOINT ["/usr/local/bin/racknerd-exporter"]
