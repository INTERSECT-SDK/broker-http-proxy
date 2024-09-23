# https://github.com/LukeMathWalker/cargo-chef
# https://www.lpalmieri.com/posts/fast-rust-docker-builds/#cargo-chef for an in-depth explanation
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app
# Force rustup to sync the toolchain in the base layer, so it doesn't happen more than once.
COPY rust-toolchain.toml .
RUN cargo --version

# the checksum of recipe.json will only change if the dependency tree changes
FROM chef AS planner
ARG BIN_NAME
# does not invalidate cache in builder stage, only planner stage
COPY . .
RUN cargo chef prepare --bin ${BIN_NAME} --recipe-path recipe.json

FROM chef AS builder
ARG BIN_NAME
RUN apt update -y && apt upgrade -y && apt install -y --no-install-recommends pkg-config libssl-dev
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --bin ${BIN_NAME} --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin ${BIN_NAME}

# Final image (should be something minimal with glibc)
FROM debian:stable-slim AS runtime
ARG BIN_NAME
WORKDIR /app
RUN apt update -y && apt upgrade -y && apt install -y --no-install-recommends pkg-config libssl-dev ca-certificates
COPY --from=builder /app/target/release/${BIN_NAME} /app/bin
ENV PROXYAPP_PRODUCTION="true"
ENTRYPOINT ["/app/bin"]
