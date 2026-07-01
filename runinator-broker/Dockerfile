ARG ALPINE_VERSION=3.22
FROM rust:1.91.1-alpine${ALPINE_VERSION} AS builder
WORKDIR /workspace
RUN apk add --no-cache build-base ca-certificates
COPY . .
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN rustup install 1.91.1 && \
    rustup default 1.91.1 && \
    cargo build --release --bin runinator-broker

FROM alpine:${ALPINE_VERSION}

RUN apk add --no-cache ca-certificates libgcc \
    && addgroup -S runinator \
    && adduser -S -D -G runinator -h /home/runinator runinator

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinator-broker /usr/local/bin/runinator-broker
RUN chown -R runinator:runinator /usr/local/bin/runinator-broker
RUN chmod -R 775 /usr/local/bin/runinator-broker

ENV RUST_LOG=info \
    RUST_BACKTRACE=1 \
    RUNINATOR_BROKER_TRANSPORT=tcp \
    RUNINATOR_BROKER_ADDR=0.0.0.0:7070
EXPOSE 7070
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-broker"]
