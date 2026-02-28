FROM rust:slim-trixie AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev curl && rm -rf /var/lib/apt/lists/*

RUN USER=root cargo new --bin figure-8
WORKDIR /figure-8

COPY . .
RUN cargo build --release

FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    chromium \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

ENV CHROME_PATH=/usr/bin/chromium

EXPOSE 8080

COPY --from=builder /figure-8/target/release/f8server /usr/local/bin/f8server
COPY --from=builder /figure-8/target/release/f8tui /usr/local/bin/f8tui

CMD ["f8server"]
