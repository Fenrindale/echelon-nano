# syntax=docker/dockerfile:1
FROM node:22.21.1-bookworm AS ui
WORKDIR /web
COPY web/package*.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --ignore-scripts
COPY web ./
RUN npm test && npm run build

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
COPY --from=ui /web/dist /work/ui-dist
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/work/target \
    cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && \
    cargo test --test knowledge_db --no-run --locked --message-format=json > /tmp/knowledge-artifacts.json && \
    test_binary="$(sed -n '/"name":"knowledge_db"/s/.*"executable":"\([^"]*\)".*/\1/p' /tmp/knowledge-artifacts.json | tail -n 1)" && \
    test -f "$test_binary" && "$test_binary" --list | grep -q '^central_knowledge_transactions_and_history: test$' && \
    cp "$test_binary" /usr/local/bin/knowledge-db-tests

FROM checks AS build
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/work/target \
    cargo build --workspace --release --locked && cp target/release/echelon /usr/local/bin/echelon

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/echelon /usr/local/bin/echelon
COPY --from=ui /web/dist /app/ui
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/echelon"]
CMD ["serve"]

FROM runtime AS knowledge-tests
COPY --from=checks /usr/local/bin/knowledge-db-tests /usr/local/bin/knowledge-db-tests
ENTRYPOINT ["/usr/local/bin/knowledge-db-tests"]
CMD ["--ignored", "--nocapture", "--test-threads=1"]

FROM runtime AS worker
USER 0:0
RUN apt-get update && apt-get install -y --no-install-recommends python3 git libseccomp2 && rm -rf /var/lib/apt/lists/* && mkdir -p /root/.broker /jobs && chmod 0700 /root /root/.broker
EXPOSE 9090
CMD ["worker"]

FROM node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205 AS mcp-host
RUN apt-get update && apt-get install -y --no-install-recommends python3 python3-pip ca-certificates && rm -rf /var/lib/apt/lists/* && mkdir -p /root/.broker /packages && chmod 0700 /root /root/.broker
COPY crates/echelon-core/src/mcp_host.py /app/mcp_host.py
RUN python3 -c "import ast,pathlib; ast.parse(pathlib.Path('/app/mcp_host.py').read_text())"
ENTRYPOINT ["python3", "/app/mcp_host.py"]
