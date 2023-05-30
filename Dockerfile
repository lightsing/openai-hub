FROM rust:latest as builder

WORKDIR /openai-hub

COPY Cargo.toml /openai-hub/Cargo.toml
COPY openai-hubd/Cargo.toml /openai-hub/openai-hubd/Cargo.toml
COPY openai-hub-core/Cargo.toml /openai-hub/openai-hub-core/Cargo.toml
COPY openai-hub-jwt-token-gen/Cargo.toml /openai-hub/openai-hub-jwt-token-gen/Cargo.toml

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/openai-hub/target \
    set -e && \
    mkdir -p ./openai-hubd/src && echo "fn main() {}" > ./openai-hubd/src/main.rs && \
    mkdir -p ./openai-hub-core/src && echo "fn main() {}" > ./openai-hub-core/src/main.rs && \
    mkdir -p ./openai-hub-jwt-token-gen/src && echo "fn main() {}" > ./openai-hub-jwt-token-gen/src/main.rs && \
    cargo build --release --all-features && \
    rm -f ./openai-hubd/src/main.rs && \
    rm -f ./openai-hub-core/src/main.rs && \
    rm -f ./openai-hub-jwt-token-gen/src/main.rs

COPY openai-hub-jwt-token-gen/src /openai-hub/openai-hub-jwt-token-gen/src
COPY openai-hubd/src /openai-hub/openai-hubd/src
COPY openai-hub-core/src /openai-hub/openai-hub-core/src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/openai-hub/target \
    set -e && \
    touch ./openai-hubd/src/main.rs ./openai-hub-core/src/lib.rs ./openai-hub-jwt-token-gen/src/main.rs && \
    cargo build --release --all-features && \
    mkdir build && \
    cp /openai-hub/target/release/openai* build/

FROM debian:11-slim

WORKDIR /opt/openai-hub

RUN mkdir -p /opt/openai-hub
COPY --from=builder /openai-hub/build/openai-hubd /opt/openai-hub/
COPY --from=builder /openai-hub/build/openai-hub-jwt-token-gen /opt/openai-hub/
COPY config.toml acl.toml /opt/openai-hub/

CMD ["/opt/openai-hub/openai-hubd"]
