# Local development and runtime

Use this guide to start, operate, and inspect a local Runinator stack. It covers the supervisor-based workflow, authentication, runtime topology, transport choices, and the cross-platform `xtask` alternative.

## Prerequisites

- Rust toolchain with Cargo.
- Docker with Compose if using the local observability helper.
- kubectl if deploying to Kubernetes or launching the UI against a K8s stack.
- pnpm if you want to build or run the Tauri `runinator-command-center` app.

## Run Locally

The quickest path on macOS/Linux is:

```bash
bash scripts/run-local.sh start
```

To start the same supervisor stack with local OTLP export, Jaeger, and
Prometheus already wired up:

```bash
bash scripts/run-local.sh observe
```

That command starts the checked-in Docker Compose observability stack, sets
`OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318` for the supervisor daemon
and its child services, then starts the normal local Runinator processes.

That checked-in supervisor config defaults to SQLite, but the same local loop can
target a server database without editing JSON:

```bash
RUNINATOR_DATABASE=mysql \
RUNINATOR_DATABASE_URL='mysql://runinator:runinator@127.0.0.1:3306/runinator' \
bash scripts/run-local.sh start
```

Use `RUNINATOR_DATABASE=postgres` with a `postgres://` or `postgresql://` URL to
point the same loop at Postgres instead.

That script runs `cargo build --workspace`, starts the supervisor in daemon mode, and prints process status. The web API listens at:

```text
http://127.0.0.1:8080/
```

The supervisor also starts `runinator-adapter-host` on `127.0.0.1:8790`, which
verifies orchestration webhook deliveries and performs adapter polling. It binds
loopback only, deliberately, because it is the process that loads adapter code.
The web service reaches it through `RUNINATOR_ADAPTER_HOST_URL` and shares its
`RUNINATOR_ADAPTER_HOST_TOKEN`; both are set in `runinator-supervisor.json`. Point
`RUNINATOR_ADAPTER_PLUGIN_PATHS` at a directory of built adapter libraries to load
dynamic adapter kinds alongside the built-in GitHub, Jira, and generic-webhook
ones. Without this process running, every orchestration adapter surface fails.

### API reference (OpenAPI)

The web service generates an OpenAPI 3.1 document automatically from `utoipa`
annotations on its handlers and serves it at:

- `http://127.0.0.1:8080/openapi.json` — the raw spec
- `http://127.0.0.1:8080/docs` — an interactive Scalar reference

Both are public (reachable without a credential). To document an endpoint, add a
`#[utoipa::path(...)]` attribute to its handler and list the handler in the
`paths(...)` set in `runinator-ws/src/openapi/mod.rs`; derive `ToSchema` on any struct
referenced by `body = ...`. Endpoints without an annotation still work — they are
simply absent from the spec until annotated, so coverage can grow incrementally.

Useful local commands:

```bash
bash scripts/run-local.sh start
bash scripts/run-local.sh foreground
bash scripts/run-local.sh status
bash scripts/run-local.sh watch
bash scripts/run-local.sh logs
bash scripts/run-local.sh logs --process web-service
bash scripts/run-local.sh logs-watch --lines 40
bash scripts/run-local.sh observe
bash scripts/run-local.sh observe-foreground
bash scripts/run-local.sh observability-start
bash scripts/run-local.sh observability-status
bash scripts/run-local.sh observability-logs
bash scripts/run-local.sh observability-stop
bash scripts/run-local.sh sync
bash scripts/run-local.sh dev
bash scripts/run-local.sh smoke-sync
bash scripts/run-local.sh ui
bash scripts/run-local.sh stop
bash scripts/run-local.sh restart
```

The supervisor runs `runinatorctl workflows apply` once per pack configured in `runinator-supervisor.json`, so those workflow packs are pushed into the API after the web service starts. The checked-in local config imports all three packs under `packs/` — `packs/sdlc/sdlc.rrx`, `packs/hello-world/hello-world.rrx`, and the `packs/creds-sync` directory — compiling the referenced `.rrx` files before sending each bundle to the API. The `creds-sync` workflows require a `runner=desktop` worker. The desktop agent advertises that label by default, so its scheduled runs use the local desktop agent; without a matching connected worker, they park then fail (see `packs/creds-sync/README.md`). It also advertises `127.0.0.1` for the web service, waker, and worker, and gives the waker and worker stable local instance ids so the replicas list shows host/IP/version data instead of blank fields on restart. Built-in provider metadata is seeded by the web service from the provider catalog on startup. If the stack is already running and you want another sync, run:

```bash
bash scripts/run-local.sh sync
```

The checked-in local supervisor config also seeds a bootstrap admin user into an empty database on first start:

```text
username: admin
password: admin
```

That seed happens even while HTTP auth is still disabled by default, so the usual local stack keeps working unchanged. If you later enable `RUNINATOR_AUTH_ENABLED=true` for the web service, you can immediately log in with that account and rotate it.

The same bootstrap step also seeds a dev-only service API key and feeds it to
the supervisor-managed development worker, one-shot `runinatorctl workflows apply`,
and the `bash scripts/run-local.sh sync|dev|smoke-sync` helpers. That means the
default local stack continues to work unchanged with auth off, and starts
working against an auth-enabled local web service without hand-editing
`runinator-supervisor.json` or exporting extra env vars.

When auth is enabled, store a local CLI session with:

```bash
runinatorctl login
```

That prompts for a username and password, or takes them from the global `--username` option and
the `RUNINATOR_USERNAME`/`RUNINATOR_PASSWORD` environment variables. Those globals work on any
command, so a one-shot invocation or the console signs in on demand without a separate `login`
step:

```bash
RUNINATOR_PASSWORD=… runinatorctl --username admin status
```

Keep the password in the environment rather than on the command line so it stays out of shell
history and the process listing.

`runinatorctl` will refresh that session automatically on later commands and will ask you to log in before calling an auth-enabled server when no valid local session, credentials, or `--api-key` is available. Remove the stored session with:

```bash
runinatorctl logout
```

The local supervisor path runs `runinator-bootstrap` before `runinator-ws`, so
schema/auth bootstrap stays outside the web-service binary even in local
development.

Once authenticated, requests are authorized on two axes, both enforced
backend-side: named **capabilities** (a documented catalog of platform/org
privileges) gate privileged handlers, and **resource grants** (View/Run/Edit/Own)
gate individual workflows and pipelines — list responses are scoped to the
workflows the caller can see, and creators are stamped as owners. The command
center hides nav/panels and disables actions the caller lacks (via `GET /auth/me`),
but that never replaces backend enforcement. See [`docs/permissions.md`](../permissions.md)
for the full model.

For rapid REXRAP development, keep a pack compiling and re-importing on every save:

```bash
bash scripts/run-local.sh dev
```

Pass `--run` to create and watch a workflow run after each successful import:

```bash
bash scripts/run-local.sh dev --run "SDLC: Development"
```

When you only need to prove the local ws/waker/worker wiring with a tiny import
and one console action, use the hello-world smoke pack:

```bash
bash scripts/run-local.sh smoke-sync
```

You can also run the supervisor directly:

```bash
cargo build --workspace
cargo run -p runinator-supervisor -- start
cargo run -p runinator-supervisor -- status
cargo run -p runinator-supervisor -- restart
cargo run -p runinator-supervisor -- stop
```

This uses `runinator-supervisor.json` to start:

- `runinator-broker`
- `runinator-blob`
- `runinator-ws`
- `runinator-waker`
- `runinator-worker`
- `runinatorctl workflows apply` (one-shot pack import)

The default worker configuration processes up to four actions concurrently. Tune
`--max-concurrent-actions` when long-running actions should not block unrelated
workflow action pickup.

### On-demand nodes

Provisionable node kinds can be spun up and scaled down on demand through the web
service's pluggable provisioner. Two backends are available: `supervisor` (adds
dynamic local processes through the running `runinator-supervisor` control queue)
and `kubernetes` (scales the backing Deployments via kube-rs; the ws image
must be built with `--features kubernetes` and the `runinator-ws-provisioner`
RBAC role applied). Enable a backend with `RUNINATOR_PROVISIONER_SUPERVISOR_ENABLED`
or `RUNINATOR_PROVISIONER_K8S_ENABLED`.

Each backend is configured per node kind, and the Node Pools panel lists the
provisionable kinds (`worker`, `waker`, `webservice`, and `postgres`);
kinds without a template/deployment on a backend show as non-manageable rows so a
newly added kind is always visible and becomes scalable the moment it is wired up.
The supervisor backend reads a spawn template per kind from
`RUNINATOR_PROVISIONER_SUPERVISOR_<KIND>` (e.g. `..._WORKER`, `..._WAKER`,
JSON `{ "command", "args", "env", "cwd" }`). The kubernetes
backend reads a deployment name per kind from
`RUNINATOR_PROVISIONER_K8S_<KIND>_DEPLOYMENT` (`worker`/`waker`/`ws` default to
`runinator-worker`/`-waker`/`-ws`, other kinds are opt-in).

Drive it from the CLI or the command center's Node Pools panel (Replicas view):

```bash
runinatorctl nodes list
runinatorctl nodes spin-up --backend supervisor --kind worker --count 2
runinatorctl nodes scale --backend kubernetes --kind worker --desired 5
runinatorctl nodes scale --backend kubernetes --kind webservice --desired 3
runinatorctl nodes stop --backend supervisor --node prov-worker-<id>
```

The Kubernetes provisioner can also observe and scale `postgres` when
`RUNINATOR_PROVISIONER_K8S_POSTGRES_STATEFULSET` is set, but scale-out above
one replica is intentionally blocked unless
`RUNINATOR_PROVISIONER_K8S_POSTGRES_SCALE_OUT_ENABLED=true` is also set. Only
enable that for a replication-aware Postgres topology with safe connection
routing, such as an operator-managed primary/replica cluster fronted by
PgBouncer. The checked-in `runinator-postgres` manifest is a single-primary
development StatefulSet and should not be scaled out as-is.

The continuation-driven graph interpreter lives in `runinator-runtime`; its
`WorkflowMachine` treats each durable cursor as a schedulable fiber and delegates
bookkeeping to a `WorkflowHost`. `runinator-engine` drives it over the broker.
The engine publishes scheduled work on the `wake` channel, and the
`runinator-waker` (a small, broker-only timer/relay) sleeps until each wake is
due and then publishes the settle it carries on the `ingress` channel for the
engine to consume. That makes the waker the timer backend for the workflow VM:
any effect that completes at a known future instant — a `wait`, an approval
expiry, a gate deadline, a debounce — is armed as a wake rather than held open
in an engine task, so a run can sleep for a week without occupying anything but
a queued message.

The durable orchestration engine — the workflow VM driver and effect dispatcher,
the effect-result consumer and infrastructure-effect host, trigger and agent-directive
publishers, plus replica/usage/metrics/notification maintenance — lives in the
`runinator-engine` library crate. `runinator-engine-worker` is only an
optional host for that engine; it does not execute provider actions and it no
longer runs the removed legacy reducer/action-dispatch loops. The engine can run in either
of two topologies. By default `runinator-ws` embeds it in-process
(`RUNINATOR_WS_RUN_ENGINE=true`), so the single-process local/dev/supervisor
stack needs no engine-worker process. Setting `RUNINATOR_WS_RUN_ENGINE=false`
makes ws serve HTTP/WebSocket only and offloads the engine to one or more standalone
`runinator-engine-worker` processes that talk to the same database and broker
directly (not the ws HTTP API), so HTTP replicas and engine replicas scale
independently. The engine is multi-replica safe: durable claims/leases
(`FOR UPDATE SKIP LOCKED`), shared-group result/ingress consumers, broker-deduped
wakes, and an idempotent per-window usage sampler let any number of
`runinator-engine-worker` (and/or engine-embedding ws) replicas run
active/active. Engine workers announce themselves as `background` replicas through broker ingress and appear in
the fleet/replica view. The Kubernetes base (`deploy/k8s/base`) and
`deploy/docker-compose.yml` ship the split topology; flip ws back to
`RUNINATOR_WS_RUN_ENGINE=true` and drop the engine-worker Deployment to fold it back
in-process. The waker holds no state and reaches the engine only over the
broker, so multiple waker replicas can run active/active. SQLite remains
the default for simple local development and single-process stacks. MariaDB and
Postgres are also supported for local development when you want a server-backed
database, and Postgres remains the intended path for multi-replica deployments.
All three run the same persistence test suite; see the dialect-parity section of
`AGENTS.md` for how to exercise the server-backed engines locally.

The local stack uses the built-in broker over raw TCP by default. The standalone
broker can also serve the same broker contract over HTTP by setting
`RUNINATOR_BROKER_TRANSPORT=http`; HTTP clients must use an endpoint like
`http://127.0.0.1:7070/`, while TCP clients use `127.0.0.1:7070`.
Kafka and RabbitMQ are available as feature-gated direct backends for the
waker, worker, web service, engine worker, and archiver. Build those binaries with `--features kafka`
or `--features rabbitmq`, set `--broker-backend kafka|rabbitmq`, use
`--broker-endpoint` for Kafka bootstrap servers or the RabbitMQ AMQP URI, and
override `--broker-effect-topic`, `--broker-infrastructure-effect-topic`,
`--broker-effect-result-topic`, `--broker-control-topic`, `--broker-wake-topic`,
or `--broker-ingress-topic` when not using the default `runinator.*`
topics/queues.
Do not scale the built-in `runinator-broker` process horizontally: each instance
has its own in-memory queue. For multi-broker high availability, run Kafka or
RabbitMQ and point every web-service/engine-worker, waker, worker, and archiver instance at the same
shared broker topics or queues.

Every non-web-service runtime announces availability, heartbeats, and clean shutdown to the
engine over broker `ingress`; web services write their own replica record as part of startup.
Worker-originated control requests travel to the engine over that same broker
`ingress` channel; API-originated `cancel`/`pause`/`resume` requests pass through
the engine and reach workers over the `control` channel. There is no direct
worker-to-waker channel.

Local runtime files are written under `~/.runinator/` by default. When using
SQLite, this includes the database at `~/.runinator/runinator.db` (which also
holds config and secrets in the `settings` table, with each value encrypted at rest by
`RUNINATOR_CREDENTIAL_KEY`), application logs under `~/.runinator/logs/`, and
supervisor state under `~/.runinator/supervisor/`.
The local supervisor runs `runinatorctl workflows apply` against the pack at
`packs/sdlc/sdlc.rrx`.
Child process stdout and stderr are collected under
`~/.runinator/supervisor/logs/` with one file per process start:

```text
YYYY-MM-DDTHH-MM-SS.mmmZ__process-name__attempt-N.log
```

Each file includes a supervisor start marker with the exact configured process name, command, and working directory, then the app's normal stdout/stderr output.

`watch` refreshes the status table. Use `logs-watch` or `logs --watch` to refresh log tails.

Use the supervisor log tail command to inspect the latest active log files:

```bash
cargo run -p runinator-supervisor -- logs
cargo run -p runinator-supervisor -- logs --process web-service --lines 100
cargo run -p runinator-supervisor -- logs --watch --lines 40
```

### Per-runtime debugging dashboard

The web service, standalone engine worker, worker, waker, and desktop agent each accept `--tui`
for a small full-screen local dashboard. It shows current work and its age/deadline, process and
host CPU/RAM, network and disk throughput, the component's existing low-cardinality
work/transport metrics, and the three most recent log lines at the bottom. The web-service
dashboard also includes the embedded engine whenever `--run-engine` is enabled (the default).

```bash
# Direct local transports
target/debug/runinator-ws --tui
target/debug/runinator-engine-worker --tui
target/debug/runinator-worker --tui
target/debug/runinator-waker --tui
target/debug/runinator-desktop-agent --tui

# A worker reaching a broker only through an already-exposed web-service relay
RUNINATOR_API_KEY='…' target/debug/runinator-worker --tui \
  --broker-mode relay --api-base-url http://localhost:8081/
```

The dashboard needs an interactive terminal; a piped or supervisor-daemon invocation falls back to
normal logs. While it is open, stdout logging is kept in the normal log file so it cannot corrupt
the display. Press `q`, `Esc`, or `Ctrl-C` to request the process's usual graceful shutdown.

## Cross-platform Local Run (xtask)

`xtask` is a plain Rust binary (`cargo run -p xtask -- <subcommand>`) that builds the
workspace and starts the local stack against the same checked-in
`runinator-supervisor.json` that `bash scripts/run-local.sh` uses, identically on Windows,
macOS, or Linux, with no PowerShell or Bash dependency:

```bash
cargo run -p xtask -- local up
```

This builds the workspace (unless `--skip-build`), makes sure the console plugin is
copied into `~/.runinator/plugins/` where the worker looks for it by default, then runs
`runinator-supervisor --config runinator-supervisor.json start --foreground` against the
`target/debug` binaries in place. There is only one local supervisor config either way you
start it. Stop it with `Ctrl+C`.

To run that same local stack against MariaDB, select the backend and pass a
MySQL-compatible URL (these become `RUNINATOR_DATABASE`/`RUNINATOR_DATABASE_URL`
environment variables for the web-service process, the same convention `bash
scripts/run-local.sh` documents above):

```bash
cargo run -p xtask -- local up \
  --database mariadb \
  --database-url 'mysql://runinator:runinator@127.0.0.1:3306/runinator'
```

`--database postgres` works the same way with a Postgres URL. SQLite continues
to use `--database-path` (defaults to `~/.runinator/runinator.db`). `cargo run
-p xtask -- build` on its own just builds the workspace plus the host-only
credential tools (`tools/keychain-export`, `tools/runinator-secret-sync`)
without starting anything.
