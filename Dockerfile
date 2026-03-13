FROM rust:latest AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY postgraph-server/ postgraph-server/

RUN cargo build --release --package postgraph-server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/postgraph-server /usr/local/bin/postgraph-server
COPY --from=builder /app/postgraph-server/migrations /app/migrations

ENV RUST_LOG=postgraph_server=info,tower_http=info

EXPOSE 8000
CMD ["postgraph-server"]
