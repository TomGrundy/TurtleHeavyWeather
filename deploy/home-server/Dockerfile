FROM rust:1.100-bookworm AS builder

WORKDIR /app
RUN rustup target add wasm32-unknown-unknown && cargo install --locked trunk

COPY . .
RUN cargo build --release -p thw-server
RUN cd frontend && THW_TRANSPORT=api trunk build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/thw-server /usr/local/bin/thw-server
COPY --from=builder /app/frontend/dist /app/static
COPY data /app/data

ENV THW_BIND_ADDR=0.0.0.0:8080
ENV THW_DB_PATH=/var/lib/turtleheavyweather/weather.db
ENV THW_NODES_PATH=/app/data/au_nodes.json
ENV THW_LOCATIONS_PATH=/app/data/locations_index.json
ENV THW_STATIC_DIR=/app/static
ENV THW_SYNC_INTERVAL_MINS=60

EXPOSE 8080
CMD ["thw-server"]
