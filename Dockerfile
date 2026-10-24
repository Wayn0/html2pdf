# syntax=docker/dockerfile:1
FROM rust:slim-bookworm AS build
WORKDIR /build
# ring's TLS implementation needs a C compiler at build time only.
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
COPY --from=build /build/target/release/html2pdf /usr/local/bin/html2pdf
WORKDIR /work
ENTRYPOINT ["html2pdf"]
CMD ["--help"]
