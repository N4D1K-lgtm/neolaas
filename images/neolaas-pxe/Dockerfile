FROM rust:latest AS chef

RUN cargo install cargo-chef

WORKDIR /app
COPY . .

RUN cargo chef prepare --recipe-path recipe.json

FROM rust:latest AS cacher

RUN cargo install cargo-chef

WORKDIR /app
COPY --from=chef /app/recipe.json recipe.json

RUN cargo chef cook --release --recipe-path recipe.json

FROM rust:latest AS builder

WORKDIR /app

COPY --from=cacher /app/target target

COPY . .

RUN cargo build --release --bin neolaas-pxe

FROM debian:trixie-slim AS runtime

WORKDIR /app

# Create boot files directory
RUN mkdir -p /var/lib/neolaas/boot

COPY --from=builder /app/target/release/neolaas-pxe /usr/local/bin/

ENTRYPOINT ["/usr/local/bin/neolaas-pxe"]
