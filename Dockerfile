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
RUN apt-get update -qq && apt install -y --no-install-recommends \
  pkg-config \
  libssl-dev \
  && apt-get clean && rm -rf /var/lib/apt/lists /var/cache/apt/archives
# Create the user and group files to run the binary as an unprivileged user.
RUN mkdir /user && \
    echo 'nobody:x:65534:65534:nobody:/:' > /user/passwd && \
    echo 'nobody:x:65534:' > /user/group
COPY --from=planner /app/recipe.json recipe.json
# strictly use static linking, but use glibc
# NOTE: when using these flags, you must explicitly specify a build target
# TODO - realistically we should use MUSL instead of GLIBC to create a static binary, however the MUSL allocator is slow and should be replaced (i.e. https://www.tweag.io/blog/2023-08-10-rust-static-link-with-mimalloc/)
ENV RUSTFLAGS='-C target-feature=+crt-static'
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --bin ${BIN_NAME} --recipe-path recipe.json --target x86_64-unknown-linux-gnu
# Build application
COPY . .
RUN cargo build --release --bin ${BIN_NAME} --target x86_64-unknown-linux-gnu

# final image, as small as possible
FROM scratch AS runtime
ARG BIN_NAME
WORKDIR /app
# Import user and group files from the build stage.
COPY --from=builder /user/group /user/passwd /etc/
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/${BIN_NAME} /app/bin
ENV PROXYAPP_PRODUCTION="true"
USER nobody:nobody
ENTRYPOINT ["/app/bin"]
