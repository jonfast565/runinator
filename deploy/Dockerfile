# syntax=docker/dockerfile:1
#
# unified build for all runinator rust service images. the builder stage
# compiles every binary (and the console plugin) in a single cargo invocation
# so shared dependencies are compiled once instead of once per image. each
# service has its own thin final stage; select it with `--target <service>`:
#
#   docker build -f deploy/Dockerfile --target ws        -t runinator-ws:dev        .
#   docker build -f deploy/Dockerfile --target waker     -t runinator-waker:dev     .
#   docker build -f deploy/Dockerfile --target worker    -t runinator-worker:dev    .
#   docker build -f deploy/Dockerfile --target archiver  -t runinator-archiver:dev  .
#   docker build -f deploy/Dockerfile --target ctl       -t runinator-ctl:dev       .
#   docker build -f deploy/Dockerfile --target bootstrap -t runinator-bootstrap:dev .
#
# with BuildKit (the default) the builder stage is cached across these target
# builds, so the expensive compile runs once for the whole set.

# ---- shared builder: compile every binary once ----
ARG ALPINE_VERSION=3.22
FROM rust:1.91.1-alpine${ALPINE_VERSION} AS builder
WORKDIR /workspace
RUN apk add --no-cache build-base ca-certificates
COPY . .
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN rustup install 1.91.1 && \
    rustup default 1.91.1 && \
    cargo build --release \
        -p runinator-ws \
        -p runinator-worker \
        -p runinator-archiver \
        -p runinator-waker \
        -p runinator-ctl \
        -p runinator-bootstrap \
        -p runinator-plugin-console \
        --features runinator-ws/rabbitmq,runinator-ws/kubernetes,runinator-worker/rabbitmq,runinator-waker/rabbitmq

# ---- shared runtime base ----
FROM alpine:${ALPINE_VERSION} AS runtime-base
RUN apk add --no-cache ca-certificates libgcc \
    && addgroup -S runinator \
    && adduser -S -D -G runinator -h /home/runinator runinator
WORKDIR /home/runinator
ENV RUST_LOG=info \
    RUST_BACKTRACE=1

# ---- ws ----
FROM runtime-base AS ws
COPY --from=builder /workspace/target/release/runinator-ws /usr/local/bin/runinator-ws
RUN chown -R runinator:runinator /usr/local/bin/runinator-ws \
    && chmod -R 775 /usr/local/bin/runinator-ws
EXPOSE 8080
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-ws"]

# ---- waker ----
FROM runtime-base AS waker
COPY --from=builder /workspace/target/release/runinator-waker /usr/local/bin/runinator-waker
RUN chown -R runinator:runinator /usr/local/bin/runinator-waker \
    && chmod -R 775 /usr/local/bin/runinator-waker
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-waker"]

# ---- worker ----
FROM runtime-base AS worker
RUN mkdir -p /home/runinator/.runinator/plugins
COPY --from=builder /workspace/target/release/runinator-worker /usr/local/bin/runinator-worker
COPY --from=builder /workspace/target/release/libruninator_plugin_console.so /home/runinator/.runinator/plugins/
RUN chown -R runinator:runinator /usr/local/bin/runinator-worker /home/runinator/.runinator \
    && chmod -R 775 /usr/local/bin/runinator-worker
VOLUME ["/data"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-worker"]

# ---- archiver ----
FROM runtime-base AS archiver
RUN mkdir -p /var/lib/runinator/archive \
    && chown -R runinator:runinator /var/lib/runinator
COPY --from=builder /workspace/target/release/runinator-archiver /usr/local/bin/runinator-archiver
RUN chown -R runinator:runinator /usr/local/bin/runinator-archiver \
    && chmod -R 775 /usr/local/bin/runinator-archiver
VOLUME ["/var/lib/runinator/archive"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-archiver"]

# ---- ctl: short-lived pack importer ----
FROM runtime-base AS ctl
RUN mkdir -p /home/runinator/.runinator/workflows
COPY --from=builder /workspace/target/release/runinatorctl /usr/local/bin/runinatorctl
COPY packs/sdlc/sdlc.wdlp /home/runinator/.runinator/workflows/sdlc.wdlp
COPY packs/sdlc/settings.wdls /home/runinator/.runinator/workflows/settings.wdls
COPY packs/sdlc/wdl /home/runinator/.runinator/workflows/wdl
RUN chown -R runinator:runinator /usr/local/bin/runinatorctl /home/runinator/.runinator \
    && chmod -R 775 /usr/local/bin/runinatorctl
ENV RUNINATOR_API_BASE_URL=http://127.0.0.1:8080/
USER runinator
# short-lived: compile and import the workflow pack, then exit. override the final argument to
# import a different pack, or pass other runinatorctl subcommands.
ENTRYPOINT ["/usr/local/bin/runinatorctl"]
CMD ["workflows", "apply", "/home/runinator/.runinator/workflows/sdlc.wdlp"]

# ---- bootstrap ----
FROM runtime-base AS bootstrap
COPY --from=builder /workspace/target/release/runinator-bootstrap /usr/local/bin/runinator-bootstrap
RUN chmod 755 /usr/local/bin/runinator-bootstrap
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-bootstrap"]
