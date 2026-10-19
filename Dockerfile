# syntax=docker/dockerfile:1
FROM rust:1.90-bookworm AS source
WORKDIR /work
COPY Cargo.toml rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations

FROM source AS lock
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo generate-lockfile
FROM scratch AS lock-export
COPY --from=lock /work/Cargo.lock /Cargo.lock

FROM source AS formatted
RUN cargo fmt --all
FROM scratch AS format-export
COPY --from=formatted /work/crates /crates

FROM source AS checks
COPY Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/work/target \
    cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked

FROM checks AS build
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/work/target \
    cargo build --workspace --release --locked && cp target/release/echelon /usr/local/bin/echelon

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/echelon /usr/local/bin/echelon
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/echelon"]
CMD ["serve"]
