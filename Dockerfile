FROM rust:1.72-bookworm as chef
RUN cargo install cargo-chef cargo-zigbuild
RUN apt update && apt install -y python3-pip libssl-dev && rm -rf /var/lib/apt/lists/*
RUN pip3 install --break-system-packages ziglang
WORKDIR openai-hub

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /openai-hub/recipe.json recipe.json
RUN cargo chef cook --zigbuild --workspace --release --recipe-path recipe.json
COPY . .
RUN cargo zigbuild --release --all-features && \
    mkdir build && \
    cp /openai-hub/target/release/openai* build/

FROM debian:bookworm-slim AS runtime-base
RUN apt update && apt install -y libssl3 && rm -rf /var/lib/apt/lists/*

FROM runtime-base
WORKDIR /opt/openai-hub
RUN mkdir -p /opt/openai-hub
COPY --from=builder /openai-hub/build/openai-hubd /opt/openai-hub/
COPY --from=builder /openai-hub/build/openai-hub-jwt-token-gen /opt/openai-hub/
COPY config.toml acl.toml /opt/openai-hub/config/
CMD ["/opt/openai-hub/openai-hubd", "-c", "config/config.toml", "-a", "config/acl.toml"]
