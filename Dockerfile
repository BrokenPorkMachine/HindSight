# syntax=docker/dockerfile:1
# Build for the Docker host architecture (linux/amd64 or linux/arm64).
# Keep this version aligned with rust-toolchain.toml; Cargo.lock is mandatory.
ARG RUST_IMAGE=rust:1.98.1-bookworm
FROM ${RUST_IMAGE} AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl-dev libllvm15 python3 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY rust-toolchain.toml ./
COPY rust/ ./rust/
COPY native/macho/ ./native/macho/
RUN cargo build --manifest-path rust/Cargo.toml --locked --release -p reworkbench-worker

# Portable qualification runs inside Linux; it never executes imported binaries.
FROM build AS test
COPY scripts/test-worker.py scripts/test-binary-formats.py scripts/test-elf-pe.py ./scripts/
COPY python/reworkbench_sdk/ ./python/reworkbench_sdk/
COPY python/tests/test_decoder.py python/tests/test_native.py ./python/tests/
COPY Tests/Fixtures/arm64-integer-sample.macho ./Tests/Fixtures/
COPY Shared/WorkbenchMessages.swift ./Shared/
RUN cargo test --manifest-path rust/Cargo.toml --locked -p reworkbench-worker -p reworkbench-protocol \
    && python3 scripts/test-worker.py rust/target/release/reworker \
    && python3 scripts/test-binary-formats.py rust/target/release/reworker \
    && python3 scripts/test-elf-pe.py rust/target/release/reworker

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 libllvm15 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/rust/target/release/reworker /usr/local/bin/reworker
USER 65532:65532
WORKDIR /tmp
LABEL org.opencontainers.image.title="HindSight Linux analysis worker" \
    org.opencontainers.image.description="Static analysis worker; not a network server or execution sandbox"
# No ports, shell wrapper, Apple frameworks, or Docker socket. Runtime limits
# and the read-only input mount are applied by scripts/docker-worker.py.
ENTRYPOINT ["/usr/local/bin/reworker"]
CMD ["--self-test"]
