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
#   docker build -f deploy/Dockerfile --target ctl       -t runinator-ctl:dev       .
#   docker build -f deploy/Dockerfile --target migration -t runinator-migration:dev .
#
# with BuildKit (the default) the builder stage is cached across these target
# builds, so the expensive compile runs once for the whole set.

# ---- shared builder: compile every binary once ----
FROM rust:1.91.1-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.1 && \
    rustup default 1.91.1 && \
    cargo build --release \
        -p runinator-ws \
        -p runinator-worker \
        -p runinator-waker \
        -p runinator-ctl \
        -p runinator-migration \
        -p runinator-plugin-console \
        --features runinator-ws/rabbitmq,runinator-worker/rabbitmq,runinator-waker/rabbitmq

# ---- shared runtime base ----
FROM debian:bookworm-slim AS runtime-base
RUN groupadd --system runinator && useradd --system --gid runinator --home /home/runinator --create-home runinator \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
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

# ---- migration ----
FROM runtime-base AS migration
COPY --from=builder /workspace/target/release/runinator-migration /usr/local/bin/runinator-migration
RUN chmod 755 /usr/local/bin/runinator-migration
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-migration"]
