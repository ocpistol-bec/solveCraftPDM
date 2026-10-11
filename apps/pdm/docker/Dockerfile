# Build context: the repository root (so both the CAD workspace and the PDM
# nested workspace are visible). Run e.g.:
#   docker build -f apps/pdm/docker/Dockerfile -t solvecraft-pdm .

FROM rust:1.99-slim AS builder
WORKDIR /build
COPY . .
# Build the CAD CLI (used by the PDM worker for headless analysis/exports).
RUN cargo build --release -p solvecraft-cli
# Build the PDM server + ops CLI (separate nested workspace).
RUN cd /build/apps/pdm && cargo build --release -p pdm-server -p pdm-cli

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/solvecraft-cli /usr/local/bin/solvecraft-cli
COPY --from=builder /build/apps/pdm/target/release/pdm-server /usr/local/bin/pdm-server
COPY --from=builder /build/apps/pdm/target/release/pdm-cli /usr/local/bin/pdm-cli
ENV PDM_SOLVECRAFT_CLI=/usr/local/bin/solvecraft-cli
EXPOSE 8080
# Server runs migrations on boot, then serves the API + in-process worker.
CMD ["pdm-server"]
