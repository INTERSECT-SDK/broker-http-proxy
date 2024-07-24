# https://github.com/LukeMathWalker/cargo-chef
# https://www.lpalmieri.com/posts/fast-rust-docker-builds/#cargo-chef for an in-depth explanation
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

# the checksum of recipe.json will only change if the dependency tree changes
FROM chef AS planner
ARG BIN_NAME
# does not invalidate cache in builder stage, only planner stage
COPY . .
RUN cargo chef prepare --bin ${BIN_NAME} --recipe-path recipe.json

FROM chef AS builder
ARG BIN_NAME
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
COPY --from=builder /app/target/release/${BIN_NAME} /usr/local/bin/app
ENTRYPOINT ["/usr/local/bin/app"]
