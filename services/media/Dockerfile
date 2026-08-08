FROM rust:1.88.0-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY services/media ./services/media
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -p proofline-media --bins && \
    mkdir /out && cp /src/target/release/proofline-gateway /src/target/release/proofline-worker /src/target/release/proofline-admin /out/

FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/proofline-gateway /usr/local/bin/proofline-gateway
COPY --from=build /out/proofline-worker /usr/local/bin/proofline-worker
COPY --from=build /out/proofline-admin /usr/local/bin/proofline-admin
USER 65532:65532
ENTRYPOINT ["proofline-gateway"]
