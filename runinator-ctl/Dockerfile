FROM rust:1.91.1-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN rustup install 1.91.1 && \
    rustup default 1.91.1 && \
    cargo build --release --bin runinatorctl

FROM debian:bookworm-slim

RUN groupadd --system runinator && useradd --system --gid runinator --home /home/runinator --create-home runinator \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && mkdir -p /home/runinator/.runinator/workflows \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /home/runinator
COPY --from=builder /workspace/target/release/runinatorctl /usr/local/bin/runinatorctl
COPY packs/sdlc/sdlc.wdlp /home/runinator/.runinator/workflows/sdlc.wdlp
COPY packs/sdlc/wdl /home/runinator/.runinator/workflows/wdl
RUN chown -R runinator:runinator /usr/local/bin/runinatorctl /home/runinator/.runinator
RUN chmod -R 775 /usr/local/bin/runinatorctl

ENV RUST_LOG=info \
    RUST_BACKTRACE=1 \
    RUNINATOR_API_BASE_URL=http://127.0.0.1:8080/
USER runinator

# short-lived: compile and import the workflow pack, then exit. override the final argument to
# import a different pack, or pass other runinatorctl subcommands.
ENTRYPOINT ["/usr/local/bin/runinatorctl"]
CMD ["workflows", "apply", "/home/runinator/.runinator/workflows/sdlc.wdlp"]
