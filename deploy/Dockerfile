# syntax=docker/dockerfile:1.7
#
# unified build for all runinator rust service images. the builder stage compiles every binary in a
# single cargo invocation so the ~1100-crate dependency graph is compiled once instead of once per
# image. each service has its own thin final stage; select it with `--target <service>`:
#
#   docker build -f deploy/Dockerfile --target ws         -t runinator-ws:dev         .
#   docker build -f deploy/Dockerfile --target engine-worker -t runinator-engine-worker:dev .
#   docker build -f deploy/Dockerfile --target waker      -t runinator-waker:dev      .
#   docker build -f deploy/Dockerfile --target worker     -t runinator-worker:dev     .
#   docker build -f deploy/Dockerfile --target archiver   -t runinator-archiver:dev   .
#   docker build -f deploy/Dockerfile --target ctl        -t runinator-ctl:dev        .
#   docker build -f deploy/Dockerfile --target bootstrap  -t runinator-bootstrap:dev  .
#   docker build -f deploy/Dockerfile --target broker     -t runinator-broker:dev     .
#   docker build -f deploy/Dockerfile --target blob       -t runinator-blob:dev       .
#   docker build -f deploy/Dockerfile --target adapter-host -t runinator-adapter-host:dev .
#
# with BuildKit (the default) the builder stage layer is shared across these target builds, so the
# expensive compile runs once for the whole set.
#
# builds are incremental across source edits: cargo's registry, git checkouts, and the workspace
# target/ directory are BuildKit cache mounts, so editing one crate recompiles that crate and its
# dependents rather than the whole graph. cache-mounted paths do not survive into the layer, which
# is why the build copies its artifacts to /out inside the same RUN.
#
# binaries are statically linked (+crt-static is the default for musl targets). that means the
# worker cannot dlopen dynamic plugins, so no .so plugins ship in these images; container workers
# run the providers compiled into them. use runinator-desktop-agent or a host worker for plugins.

ARG ALPINE_VERSION=3.24
ARG RUST_VERSION=1.97.1
ARG RUNINATOR_VERSION=0.3.498

# ---- shared builder: compile every Kubernetes binary once ----
FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS builder
# BuildKit-provided; only used to key the target/ cache so a cross-arch build does not thrash the
# cache of the other arch (cargo would rebuild the world on every alternation).
ARG TARGETARCH
ARG RUNINATOR_K8S_DATABASE_BACKEND=postgres
ARG RUNINATOR_K8S_BROKER_BACKEND=rabbitmq
WORKDIR /workspace
RUN apk add --no-cache build-base ca-certificates
# strip the symbol table from the shipped binaries: it is ~33% of a static musl binary here
# (runinator-ws 90.6MB -> 60.2MB), and that weight is paid on every registry push and pod pull.
# scoped to the image build on purpose — host `cargo build --release` and the release-builds.yml
# bundles keep their symbols, so panic backtraces stay readable where people actually debug.
ENV CARGO_PROFILE_RELEASE_STRIP=symbols
COPY . .
# the registry and git caches hold downloaded sources and are arch-independent, so they stay shared.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/workspace/target,id=runinator-target-${TARGETARCH},sharing=locked \
    case "$RUNINATOR_K8S_DATABASE_BACKEND" in \
      sqlite) database_features='runinator-ws/sqlite,runinator-engine-worker/sqlite,runinator-worker/db-sqlite,runinator-archiver/sqlite,runinator-bootstrap/sqlite,runinator-ctl/db-sqlite' ;; \
      postgres) database_features='runinator-ws/postgres,runinator-engine-worker/postgres,runinator-worker/db-postgres,runinator-archiver/postgres,runinator-bootstrap/postgres,runinator-ctl/db-postgres' ;; \
      mysql|mariadb) database_features='runinator-ws/mysql,runinator-engine-worker/mysql,runinator-worker/db-mysql,runinator-archiver/mysql,runinator-bootstrap/mysql,runinator-ctl/db-mysql' ;; \
      *) echo "unsupported RUNINATOR_K8S_DATABASE_BACKEND: $RUNINATOR_K8S_DATABASE_BACKEND (expected sqlite, postgres, mysql, or mariadb)" >&2; exit 2 ;; \
    esac \
    && case "$RUNINATOR_K8S_BROKER_BACKEND" in \
      http) broker_features='runinator-ws/http,runinator-engine-worker/http,runinator-worker/http,runinator-waker/http' ;; \
      tcp) broker_features='runinator-ws/tcp,runinator-engine-worker/tcp,runinator-worker/tcp,runinator-waker/tcp' ;; \
      kafka) broker_features='runinator-ws/kafka,runinator-engine-worker/kafka,runinator-worker/kafka,runinator-waker/kafka' ;; \
      rabbitmq) broker_features='runinator-ws/rabbitmq,runinator-engine-worker/rabbitmq,runinator-worker/rabbitmq,runinator-waker/rabbitmq' ;; \
      *) echo "unsupported RUNINATOR_K8S_BROKER_BACKEND: $RUNINATOR_K8S_BROKER_BACKEND (expected http, tcp, kafka, or rabbitmq)" >&2; exit 2 ;; \
    esac \
    && cargo build --release --locked --no-default-features \
        -p runinator-ws \
        -p runinator-engine-worker \
        -p runinator-worker \
        -p runinator-archiver \
        -p runinator-waker \
        -p runinator-ctl \
        -p runinator-bootstrap \
        -p runinator-broker \
        -p runinator-blob \
        -p runinator-adapter-host \
        --features "$database_features,$broker_features,runinator-ws/kubernetes,runinator-broker/tcp,runinator-blob/server" \
    && mkdir -p /out \
    && cp target/release/runinator-ws \
          target/release/runinator-engine-worker \
          target/release/runinator-worker \
          target/release/runinator-archiver \
          target/release/runinator-waker \
          target/release/runinatorctl \
          target/release/runinator-bootstrap \
          target/release/runinator-broker \
          target/release/runinator-blob \
          target/release/runinator-adapter-host \
          /out/

# ---- shared runtime base ----
FROM alpine:${ALPINE_VERSION} AS runtime-base
ARG RUNINATOR_VERSION
LABEL org.opencontainers.image.title="Runinator" \
      org.opencontainers.image.version="${RUNINATOR_VERSION}" \
      org.opencontainers.image.vendor="Runinator"
RUN apk add --no-cache ca-certificates \
    && addgroup -S runinator \
    && adduser -S -D -G runinator -h /home/runinator runinator
WORKDIR /home/runinator
ENV RUNINATOR_LOG=info \
    RUST_BACKTRACE=1

# ---- ws ----
FROM runtime-base AS ws
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-ws /usr/local/bin/runinator-ws
EXPOSE 8080
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-ws"]

# ---- engine-worker: standalone durable orchestration engine (ws with RUNINATOR_WS_RUN_ENGINE=false
#      offloads its engine here so HTTP replicas and engine replicas scale independently) ----
FROM runtime-base AS engine-worker
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-engine-worker /usr/local/bin/runinator-engine-worker
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-engine-worker"]

# ---- waker ----
FROM runtime-base AS waker
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-waker /usr/local/bin/runinator-waker
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-waker"]

# ---- worker: built-in providers only; statically linked, so no dynamic plugin loading ----
FROM runtime-base AS worker
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-worker /usr/local/bin/runinator-worker
VOLUME ["/data"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-worker"]

# ---- archiver ----
FROM runtime-base AS archiver
RUN mkdir -p /var/lib/runinator/archive && chown -R runinator:runinator /var/lib/runinator
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-archiver /usr/local/bin/runinator-archiver
VOLUME ["/var/lib/runinator/archive"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-archiver"]

# ---- broker: standalone tcp broker for single-host deployments that skip rabbitmq ----
FROM runtime-base AS broker
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-broker /usr/local/bin/runinator-broker
ENV RUNINATOR_BROKER_TRANSPORT=tcp \
    RUNINATOR_BROKER_ADDR=0.0.0.0:7070
EXPOSE 7070
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-broker"]

# ---- blob: s3-compatible object store over the container filesystem ----
FROM runtime-base AS blob
RUN mkdir -p /var/lib/runinator/blobs && chown -R runinator:runinator /var/lib/runinator
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-blob /usr/local/bin/runinator-blob
ENV RUNINATOR_BLOB_ADDR=0.0.0.0:9000 \
    RUNINATOR_BLOB_DATA_DIR=/var/lib/runinator/blobs
EXPOSE 9000
VOLUME ["/var/lib/runinator/blobs"]
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-blob"]

# ---- adapter-host: orchestration adapter verification and polling ----
# this runs as a sidecar rather than its own service: it binds loopback only, deliberately, because
# it is the process that loads adapter code. the static musl link means it has no dynamic loader
# either, so in a container it serves the compiled-in kinds and ignores RUNINATOR_ADAPTER_PLUGIN_PATHS.
FROM runtime-base AS adapter-host
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-adapter-host /usr/local/bin/runinator-adapter-host
ENV RUNINATOR_ADAPTER_HOST_PORT=8790
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-adapter-host"]

# ---- ctl: short-lived pack importer ----
FROM runtime-base AS ctl
COPY --from=builder --chown=root:root --chmod=755 /out/runinatorctl /usr/local/bin/runinatorctl
# bake every pack in, each keeping its own subdirectory so `.rrx` source names never collide
# across packs; the import job applies each workflow-containing directory.
COPY --chown=runinator:runinator packs /home/runinator/.runinator/workflows/packs
ENV RUNINATOR_API_BASE_URL=http://127.0.0.1:8080/
USER runinator
# short-lived: compile and import a workflow pack, then exit. the default applies the SDLC pack
# directory; the `.rrx` compiler discovers its nested sources. override the final argument to
# import a different pack, or pass other runinatorctl subcommands. the k8s pack-import job
# overrides this to loop over every baked-in pack.
ENTRYPOINT ["/usr/local/bin/runinatorctl"]
CMD ["workflows", "apply", "/home/runinator/.runinator/workflows/packs/sdlc"]

# ---- bootstrap ----
FROM runtime-base AS bootstrap
COPY --from=builder --chown=root:root --chmod=755 /out/runinator-bootstrap /usr/local/bin/runinator-bootstrap
USER runinator
ENTRYPOINT ["/usr/local/bin/runinator-bootstrap"]
