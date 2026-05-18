FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY src/ src/
RUN touch src/main.rs && cargo build --release

FROM alpine:3
RUN apk add --no-cache ca-certificates
COPY --from=builder /app/target/release/dataset-gateway /usr/local/bin/
EXPOSE 8080
ENTRYPOINT ["dataset-gateway"]
