FROM rust:1.88.0-bookworm AS build
WORKDIR /build
ENV CARGO_BUILD_JOBS=2
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release --bin lilvault \
    && strip target/release/lilvault

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 lilvault \
    && useradd --uid 1000 --gid 1000 --no-create-home --home-dir /nonexistent lilvault
COPY --from=build /build/target/release/lilvault /usr/local/bin/lilvault
COPY deploy/initialize.sh /usr/local/bin/lilvault-init
RUN chmod 0755 /usr/local/bin/lilvault-init
USER 1000:1000
ENV RUST_LOG=info
EXPOSE 8737
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent --output /dev/null http://127.0.0.1:8737/readyz || exit 1
ENTRYPOINT ["/usr/local/bin/lilvault"]
CMD ["/etc/lilvault/config.toml"]
