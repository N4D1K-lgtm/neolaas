FROM rust:1.92.0-trixie AS chef

RUN cargo install cargo-chef && apt-get update && \
  apt-get -y --no-install-recommends install protobuf-compiler=3.21.12-11 && \
  apt-get clean && \
  rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cacher

RUN cargo install cargo-chef

WORKDIR /app
COPY --from=chef /app/recipe.json recipe.json

RUN cargo chef cook --release --recipe-path recipe.json

FROM chef AS builder

WORKDIR /app

COPY --from=cacher /app/target target

COPY . .

RUN cargo build --release --bin neolaas-server

FROM debian:trixie-slim AS runtime

WORKDIR /app

COPY --from=builder /app/target/release/neolaas-server /usr/local/bin/

ENTRYPOINT ["/usr/local/bin/neolaas-server"]

