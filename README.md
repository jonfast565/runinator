# runinator

Runinator is a Rust workspace for scheduling and executing tasks across a small local/distributed runtime. The local development path uses `runinator-supervisor` to run the broker, web service, waker, and worker, plus a one-shot `runinatorctl` pack import.

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

### API reference (OpenAPI)

The web service generates an OpenAPI 3.1 document automatically from `utoipa`
annotations on its handlers and serves it at:

- `http://127.0.0.1:8080/openapi.json` — the raw spec
- `http://127.0.0.1:8080/docs` — an interactive Scalar reference

Both are public (reachable without a credential). To document an endpoint, add a
`#[utoipa::path(...)]` attribute to its handler and list the handler in the
`paths(...)` set in `runinator-ws/src/openapi.rs`; derive `ToSchema` on any struct
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

The supervisor runs `runinatorctl workflows apply` once per pack configured in `runinator-supervisor.json`, so those workflow packs are pushed into the API after the web service starts. The checked-in local config imports all three packs under `packs/` — `packs/sdlc/sdlc.wdlp`, `packs/hello-world/hello-world.wdlp`, and the `packs/creds-sync` directory — compiling the referenced `.wdl` files before sending each bundle to the API. The `creds-sync` workflows require a `runner=creds-sync` worker, so on the local stack their scheduled runs park then fail unless you start such a worker (see `packs/creds-sync/README.md`). It also advertises `127.0.0.1` for the web service, waker, and worker, and gives the waker and worker stable local instance ids so the replicas list shows host/IP/version data instead of blank fields on restart. Built-in provider metadata is seeded by the web service from the provider catalog on startup. If the stack is already running and you want another sync, run:

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
the checked-in local worker, waker, one-shot `runinatorctl workflows apply`,
and the `bash scripts/run-local.sh sync|dev|smoke-sync` helpers. That means the
default local stack continues to work unchanged with auth off, and starts
working against an auth-enabled local web service without hand-editing
`runinator-supervisor.json` or exporting extra env vars.

When auth is enabled, store a local CLI session with:

```bash
runinatorctl login
```

`runinatorctl` will refresh that session automatically on later commands and will ask you to log in before calling an auth-enabled server when no valid local session or `--api-key` is available. Remove the stored session with:

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
but that never replaces backend enforcement. See [`docs/permissions.md`](docs/permissions.md)
for the full model.

For rapid WDL development, keep a pack compiling and re-importing on every save:

```bash
bash scripts/run-local.sh dev
```

Pass `--run` to create and watch a workflow run after each successful import:

```bash
bash scripts/run-local.sh dev --run "Core Team SDLC Pipeline"
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
- `runinator-ws`
- `runinator-waker`
- `runinator-worker`
- `runinatorctl workflows apply` (one-shot pack import)

The default worker configuration processes up to four actions concurrently. Tune
`--max-concurrent-actions` when long-running actions should not block unrelated
workflow action pickup.

### On-demand nodes

Nodes of every kind can be spun up and scaled down on demand through the web
service's pluggable provisioner. Two backends are available: `supervisor` (adds
dynamic local processes through the running `runinator-supervisor` control queue)
and `kubernetes` (scales the backing Deployments via kube-rs; the ws image
must be built with `--features kubernetes` and the `runinator-ws-provisioner`
RBAC role applied). Enable a backend with `RUNINATOR_PROVISIONER_SUPERVISOR_ENABLED`
or `RUNINATOR_PROVISIONER_K8S_ENABLED`.

Each backend is configured per node kind, and the Node Pools panel lists **every**
kind (`worker`, `waker`, `webservice`, `background`, `archiver`, `postgres`);
kinds without a template/deployment on a backend show as non-manageable rows so a
newly added kind is always visible and becomes scalable the moment it is wired up.
The supervisor backend reads a spawn template per kind from
`RUNINATOR_PROVISIONER_SUPERVISOR_<KIND>` (e.g. `..._WORKER`, `..._WAKER`,
`..._BACKGROUND`; JSON `{ "command", "args", "env", "cwd" }`). The kubernetes
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

The web service owns the reducer and drives workflows over the broker: it
publishes scheduled work on the `wake` channel, and the `runinator-waker` (a
small, broker-only timer/relay) sleeps until each ready node is due and then
publishes a `drive` on the `ingress` channel that the web service consumes to
advance the run.

The durable orchestration engine — the reducer plus the wake/trigger/action/
ingress loops, the result consumer, and the replica/ready-node/usage maintenance
backstops — lives in the `runinator-engine` library crate and can run in either
of two topologies. By default `runinator-ws` embeds it in-process
(`RUNINATOR_WS_RUN_ENGINE=true`), so the single-process local/dev/supervisor
stack runs everything as-is. Setting `RUNINATOR_WS_RUN_ENGINE=false` makes ws
serve HTTP/WebSocket only and offloads the engine to one or more standalone
`runinator-background-worker` processes that talk to the same database and broker
directly (not the ws HTTP API), so HTTP replicas and engine replicas scale
independently. The engine is multi-replica safe: durable claims/leases
(`FOR UPDATE SKIP LOCKED`), shared-group result/ingress consumers, broker-deduped
wakes, and an idempotent per-window usage sampler let any number of
`runinator-background-worker` (and/or engine-embedding ws) replicas run
active/active. Background workers register as `background` replicas and appear in
the fleet/replica view. The Kubernetes base (`deploy/k8s/base`) and
`deploy/docker-compose.yml` ship the split topology; flip ws back to
`RUNINATOR_WS_RUN_ENGINE=true` and drop the background Deployment to fold it back
in-process. The waker holds no state and reaches the web service only over
the broker, so multiple waker replicas can run active/active. SQLite remains
the default for simple local development and single-process stacks. MariaDB and
Postgres are also supported for local development when you want a server-backed
database, and Postgres remains the intended path for multi-replica deployments.

The local stack uses the built-in broker over raw TCP by default. The standalone
broker can also serve the same broker contract over HTTP by setting
`RUNINATOR_BROKER_TRANSPORT=http`; HTTP clients must use an endpoint like
`http://127.0.0.1:7070/`, while TCP clients use `127.0.0.1:7070`.
Kafka and RabbitMQ are available as feature-gated direct backends for the
waker, worker, and web service. Build those binaries with `--features kafka`
or `--features rabbitmq`, set `--broker-backend kafka|rabbitmq`, use
`--broker-endpoint` for Kafka bootstrap servers or the RabbitMQ AMQP URI, and
override `--broker-action-topic`, `--broker-control-topic`,
`--broker-result-topic`, `--broker-wake-topic`, or `--broker-ingress-topic` when
not using the default `runinator.*` topics/queues.
Do not scale the built-in `runinator-broker` process horizontally: each instance
has its own in-memory queue. For multi-broker high availability, run Kafka or
RabbitMQ and point every web-service, waker, and worker instance at the same
shared broker topics or queues.

Worker-originated control requests travel to the web service over the broker
`ingress` channel; the web service issues `cancel`/`pause`/`resume` to workers
over the `control` channel. There is no direct worker-to-waker channel.

Local runtime files are written under `~/.runinator/` by default. When using
SQLite, this includes the database at `~/.runinator/runinator.db` (which also
holds config and secrets in the `settings` table, with each value encrypted at rest by
`RUNINATOR_CREDENTIAL_KEY`), application logs under `~/.runinator/logs/`, and
supervisor state under `~/.runinator/supervisor/`.
The local supervisor runs `runinatorctl workflows apply` against the pack at
`packs/sdlc/sdlc.wdlp`.
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

## Cross-platform Local Run (xtask)

`xtask` is a plain Rust binary (`cargo run -p xtask -- <subcommand>`) that replaces the
old PowerShell `build.ps1` — it builds the workspace and starts the same local stack,
against the same checked-in `runinator-supervisor.json` that `bash scripts/run-local.sh`
uses, identically on Windows, macOS, or Linux, with no PowerShell or Bash dependency:

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

## Workflow Import

`runinatorctl workflows apply <path>` imports a workflow pack in one shot. The
path can be a `.wdl` file, a `.wdlp` manifest (which lists the `.wdl` files that
make up the pack, resolved relative to the manifest), a directory of `.wdl`
files, or a workflow/bundle JSON file. The local supervisor config applies
`./packs/sdlc/sdlc.wdlp`. To load local credentials and config, import a settings
bundle with `runinatorctl settings import <file>`. Each entry carries a `kind`
(`secret` — the default — or `config`) and a `value`; secret values stay
encrypted and resolve late at the worker, while config values are arbitrary JSON
read by the web service. You can seed the app-data
workflow manifest from the repository sample pack if needed:

```bash
mkdir -p ~/.runinator/workflows
cp packs/sdlc/sdlc.wdlp ~/.runinator/workflows/sdlc.wdlp
cp -R packs/sdlc/wdl ~/.runinator/workflows/wdl
```

Compiled JSON workflow packs are no longer checked in. Use `sdlc.wdlp` plus the
referenced `.wdl` sources for imports.

`runinatorctl workflows dev <path>` runs the same client-side pack compile and
compiled zip upload in a watch loop. It watches the pack manifest, referenced
`.wdl` files, adjacent settings, and an optional `--json-file`. When `--run` is
provided, it starts that workflow after each successful import and refreshes the
run detail until the run reaches a terminal state.

For the checked-in local stack only, `bash scripts/run-local.sh sync`, `dev`,
and `smoke-sync` will default `RUNINATOR_API_KEY` to the same seeded dev
service key when talking to `http://127.0.0.1:8080/` and no explicit
`RUNINATOR_API_KEY` is already set. Pointing those helpers at another stack
still requires that stack's own credentials.

For a minimal smoke import, use `./packs/hello-world/hello-world.wdlp`. It
contains one WDL workflow that runs a single built-in console action and is wired
into `bash scripts/run-local.sh smoke-sync` for an import-and-run check against
an already running local stack.

### Editor integration (language server)

`runinator-lsp` is an editor-agnostic Language Server for `.wdl` files: live diagnostics,
provider/action completion (from live service metadata), hover, formatting, and an optional
apply-on-save that imports the pack into a running web service — the editor-native counterpart of
`runinatorctl workflows dev`. Build it with `cargo build -p runinator-lsp --release` and point your
editor at the binary. See [`runinator-lsp/README.md`](runinator-lsp/README.md) for the VS Code
extension and Neovim/Zed setup. The pack compile-to-bundle logic shared by the CLI and the server
lives in the `runinator-pack` crate.

Workflow syntax now includes richer declarative control-flow nodes:

- `switch` routes by ordered cases and an optional default target.
- `parallel` starts branch roots, with branch nodes returning to a `join`.
- `join` waits for named upstream nodes using `all`, `any`, or `first_success`.
- `try` runs a body, optional catch, and optional finally node; those nodes transition back to the `try` controller.
- `map` runs one target node for each resolved item and exposes the current item under `workflow.state.map`.
- `race` starts branch roots until one satisfies the winner policy, then cancels the still-running losing branches (their latest non-terminal node run is marked `Canceled`).
- `emit` records structured node output without calling a provider.
- `reentry` allows explicit bounded cycles back to a node and can route to `on_exhausted`.

#### Triggers and workflow chaining

Workflows declare triggers in the WDL header, materialized from `metadata.triggers`
on import (pack-managed, `metadata.managed_by = "wdl"`):

- `trigger cron "<expr>"` schedules the workflow.
- `trigger on_success | on_failure | on_complete workflow "<name>"` chains another
  workflow: when this run reaches that terminal state, the named target starts a new
  run. `on_failure` matches `Failed`/`TimedOut` (not a manual `Cancel`).

```wdl
trigger cron "0 * * * *"
trigger on_success workflow "Downstream Report"
```

Chaining is event-driven from the reducer's terminal settle (not the best-effort
`events` channel), fired exactly once per (trigger, source-run) via a durable
dedupe table, and cycle-bounded by a `chain_depth` cap. Only top-level runs fan out
chains — subflow/map children do not. Chaining does **not** replace `subflow`: a
subflow is a synchronous child with a return path, while a chain starts an
independent downstream run.

The command center's top-level **Pipelines** tab visualizes chains as a DAG — one
node per workflow, one edge per chained trigger — and lets you author them by
dragging between workflows, editing an edge's `on` selector, or enabling/disabling
and deleting chains through the normal trigger CRUD.

WDL references resolve runtime values into action arguments. Alongside `params.*`,
`prev.*`, `run.*`, and bare node-output names, two roots read from the unified
settings store:

- `config.<scope>.<name>` — non-sensitive JSON, resolved eagerly by the web
  service. It interpolates freely (e.g. `"${config.api.base}/v2"`) and can drill
  into stored JSON (`config.api.settings.url`).
- `secret.<scope>.<name>` — sensitive values, lowered to the `secret://scope/name`
  form and resolved late at the worker so plaintext never reaches the web
  service, database, or broker. A secret must be passed as a whole argument value
  (it cannot be interpolated mid-string).

Stored settings are typed. Config values are validated on write against a declared
JSON-schema (required once per `scope/name`, then reused for value-only updates);
a value that does not match the schema is rejected. Secrets are validated as
non-empty strings. Manage them with `runinatorctl`:

```bash
# declare + store a config value (schema required on first write)
runinatorctl settings set api base '"https://api.example.com"' \
  --kind config --schema '{ "type": "string" }'
# later value-only update reuses the stored schema
runinatorctl settings set api base '"https://api.example.com/v2"' --kind config

# store a secret (string)
runinatorctl settings set github token "ghp_xxx"

# read a value from a file instead of passing it inline
runinatorctl settings set github deploy-key --value-file ./id_ed25519

# bulk import secrets and config from a bundle file
runinatorctl settings import ./secrets.json

runinatorctl settings list            # all settings, no values
runinatorctl settings get api base --kind config
```

The import file is a `{ "secrets": [...] }` document; each entry carries
`scope`, `name`, and `value`, plus optional `kind` (`secret` or `config`) and
`schema`. Existing entries are only overwritten when an incoming `updated_at` is
strictly newer.

The v1 control-flow runtime is controller-driven and still uses one `active_node_id`.
`parallel` and `race` advance branch roots sequentially through persisted workflow state,
and `map.concurrency` is reserved for a future multi-active-node runtime. Branch/body/item
nodes should transition back to their owning `join`, `try`, `map`, or `race` controller.

## Observability

Every service binary (`ws`, `worker`, `waker`) emits structured logs to stdout and
a log file via `tracing`, filtered by `RUNINATOR_LOG` (an `EnvFilter` directive,
default `info`). The web service additionally exposes Prometheus metrics at
`/metrics`.

The `runinator-desktop-agent` tray app honors the same `RUNINATOR_LOG` directive at
startup and additionally renders those `tracing` records into its in-app log console,
where a **Log → Level** dropdown changes the level live (no restart) and persists it.

OpenTelemetry export is **off by default and turns on purely from the standard
`OTEL_*` environment variables** — no CLI flags or config-file options. When
`OTEL_EXPORTER_OTLP_ENDPOINT` (or a signal-specific
`OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT`) is set, each binary stands up
OTLP exporters for **traces, metrics, and logs** over OTLP HTTP/protobuf;
`OTEL_SDK_DISABLED=true` forces it off. The service name defaults to the binary
(e.g. `Runinator Web Service`) and is overridable with `OTEL_SERVICE_NAME` /
`OTEL_RESOURCE_ATTRIBUTES`.

Trace context propagates across hops using W3C `traceparent`: inbound HTTP requests
to the web service continue the caller's trace, and the reducer stamps the active
context onto each `ActionCommand` so a worker's execution span links back to the
dispatching trace. Prometheus `/metrics` remains available alongside OTLP metrics.

Each service and the broker emit runtime metrics over OTLP (and, for the web
service, also on Prometheus `/metrics`):

- **Web service** (`runinator_ws_*`): `result_events_{applied,duplicate,retried,dead_lettered}_total`,
  `result_receive_errors_total`, `handler_panics_total`, `background_loop_failures_total`,
  `ingress_{applied,retried,dead_lettered}_total`, `triggers_fired_total`, and the
  `reducer_drive_ms` histogram (reducer time per drive).
- **Worker** (`runinator_worker_*`): `actions_received_total`, `actions_completed_total`
  and the `action_duration_ms` histogram (both split by `outcome`),
  `actions_duplicate_total`, `actions_in_flight` (gauge), `control_commands_total`
  (by `kind`), and `secret_resolution_failures_total`.
- **Waker** (`runinator_waker_*`): `wakes_{received,driven,requeued}_total`,
  `drive_failures_total`, and the `wake_lead_ms` histogram (scheduling lead/lag at
  receipt).
- **Broker** (`runinator_broker_*`, emitted by every service): `operations_total` and
  the `operation_duration_ms` histogram, tagged with `backend` (in-memory/http/tcp/
  kafka/rabbitmq), `channel`, `op`, and (for the counter) `outcome`.

```bash
# point all binaries at a local OpenTelemetry Collector (OTLP/HTTP on :4318)
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
cargo run -p runinator-supervisor -- start
```

For the checked-in local supervisor flow, prefer the one-command helper:

```bash
bash scripts/run-local.sh observe
```

It starts `deploy/local-observability/compose.yaml` with:

- OpenTelemetry Collector receiving OTLP HTTP on `http://127.0.0.1:4318`
  and OTLP gRPC on `127.0.0.1:4317`.
- Jaeger at `http://127.0.0.1:16686` for traces.
- Prometheus at `http://127.0.0.1:9090` scraping the collector's re-exported
  OTLP metrics on `otel-collector:8889` plus collector self-metrics on `:8888`.
- Loki at `http://127.0.0.1:3100` receiving the collector's logs signal via
  OTLP, so structured fields the binaries set (`trace_id`, `run_id`,
  `error_code`, ...) are queryable with LogQL instead of only living in
  stdout/log files.
- Grafana at `http://127.0.0.1:3000` (anonymous admin) with Loki, Prometheus,
  and Jaeger pre-provisioned as datasources — the natural place to query logs
  and click a `trace_id` through to its Jaeger trace.

After the stack starts, run `bash scripts/run-local.sh smoke-sync` or drive a
workflow through the UI/CLI, then inspect traces in Jaeger, metrics in
Prometheus, and logs in Grafana (or `loki`, e.g. via `logcli`). Use
`bash scripts/run-local.sh observability-logs --lines 120` to inspect
collector/exporter output, and use `bash scripts/run-local.sh
observability-stop` to stop the local observability containers.

**In Kubernetes**, the `components/observability` kustomize component deploys an
OpenTelemetry Collector, Jaeger (trace UI), Prometheus (scrapes the collector),
Loki (durable/queryable log store), and Grafana (dashboards over Prometheus +
Jaeger + Loki), and points the services at the collector. It is enabled in the
`local` overlay by default; add `../../components/observability` to another
overlay's `components:` list to turn it on there (and remove it to turn otel
back off). After deploying:

```bash
# dashboards + logs — open Grafana at http://localhost:3000 (anonymous admin; "Runinator
# Overview" dashboard is provisioned, with Loki + Prometheus + Jaeger datasources wired up)
bash scripts/port-forward-grafana.sh   # or: kubectl -n runinator port-forward svc/runinator-grafana 3000:3000
# traces — open the Jaeger UI at http://localhost:16686
kubectl -n runinator port-forward svc/runinator-jaeger 16686:16686
# raw metrics — the Prometheus UI / API at http://localhost:9090
kubectl -n runinator port-forward svc/runinator-prometheus 9090:9090
# raw logql — the Loki API at http://localhost:3100
kubectl -n runinator port-forward svc/runinator-loki 3100:3100
# a copy of every signal — the collector's debug exporter
kubectl -n runinator logs deploy/runinator-otel-collector
```

Grafana's anonymous-admin login is for convenient local viewing; lock it down (set
a real admin password and disable anonymous access) before using it on a shared
cluster.

### Dead letters and audit log

Poison messages are no longer dropped silently. When a result or ingress event
cannot be applied and is given up on, the web service persists a `dead_letters`
row before acking, so failed messages have a durable record. Auth and sensitive
operations (login success/failure, authorization denials) are recorded to an
`audit_log` table. Both are exposed as admin-only endpoints (`GET /dead_letters`,
`GET /audit_log`, in the OpenAPI spec) and surfaced in the command center as
admin-gated **Dead Letters** and **Audit Log** views.

## Kubernetes

The Kubernetes manifests live under `deploy/k8s/` and are organized as a
kustomize base with two overlays:

```
deploy/k8s/
  base/                     # core manifests (namespace, services, postgres, rabbitmq, app deployments)
  overlays/local/           # k3d/minikube/kind — light replicas, default StorageClass
  overlays/prod/            # real registry + StorageClass + production resource sizing
```

The K8s stack uses **Postgres** in-cluster (StatefulSet + PVC) and **RabbitMQ**
as the broker (via the `rabbitmq` Cargo feature, baked into the ws/waker/
worker images). The standalone `runinator-broker` binary is not deployed in K8s.

Schema is applied by the `runinator-bootstrap` image, which runs the embedded
SQL bootstrap from `runinator-database/migrations/` and can also seed the first
admin account when `RUNINATOR_AUTH_BOOTSTRAP_ADMIN` is provided. By default this
only seeds into an empty user table; set `RUNINATOR_AUTH_BOOTSTRAP_ADMIN_FORCE=true`
as a break-glass to reset that admin's password on the next bootstrap even when
users already exist (recovers a locked-out admin), then unset it. The
`runinator-ws` Deployment runs bootstrap from an initContainer on every pod
start. `deploy/k8s/base/db-bootstrap-job.yaml` is kept as an optional
out-of-band ops manifest; it is not part of the default kustomize base because
Kubernetes Job pod templates are immutable across image tag changes.

The bundled pack-import Job now logs in with the bootstrap-admin credentials
before it runs `workflows apply`, so `runinator-app-secret` must carry
`RUNINATOR_BOOTSTRAP_ADMIN_USERNAME` and `RUNINATOR_BOOTSTRAP_ADMIN_PASSWORD`
alongside `RUNINATOR_AUTH_BOOTSTRAP_ADMIN`.

For non-Kubernetes environments, `runinator-bootstrap` also supports
`--database mysql` / `--database mariadb` with a `mysql://...` connection string,
in addition to the existing SQLite and Postgres modes.

#### Key rotation (two-key overlap)

Both at-rest keys support a primary + previous overlap so a key can be rotated
without invalidating live tokens or stranding stored secrets:

- **JWT signing secret.** New access tokens are always signed with
  `RUNINATOR_AUTH_JWT_SECRET` (the primary); the web service also accepts tokens
  signed with `RUNINATOR_AUTH_JWT_SECRET_PREVIOUS` on verify. To rotate: set the
  new secret as the primary and the old one as `*_PREVIOUS`, redeploy so bootstrap
  persists both, wait past the access-token TTL, then clear `*_PREVIOUS` (bootstrap
  deletes the slot) and redeploy to retire the old key.
- **Credential encryption key.** Stored settings — including the JWT signing
  secret — are encrypted at rest with `RUNINATOR_CREDENTIAL_KEY` (the primary)
  and tagged with a short key id; `RUNINATOR_CREDENTIAL_KEY_PREVIOUS`
  (comma-separated) lists prior keys still accepted on decrypt. To rotate: set the
  new key as the primary and the old one as `*_PREVIOUS`, redeploy ws,
  `POST /credentials/reencrypt` (admin) to re-tag every stored value with the new
  key, then clear `*_PREVIOUS` and redeploy. A signing secret persisted before
  encryption was added is migrated to the encrypted form on the next bootstrap.
- **Rate limiting.** On by default; set `RUNINATOR_RATE_LIMIT_ENABLED=false` to
  disable. It gates the HTTP API with an in-memory token bucket keyed by the
  authenticated principal (falling back to the connection IP). Tune it with
  `RUNINATOR_RATE_LIMIT_RPS` (sustained requests per second, default `50`) and
  `RUNINATOR_RATE_LIMIT_BURST` (bucket size, default `100`). Each ws replica limits
  independently; `/health`, `/ready`, and `/metrics` are exempt. Over-limit
  requests get `429` with a `Retry-After` header. Independently, the unauthenticated
  `/auth/login` endpoint carries an always-on per-IP brute-force throttle (a small
  burst, then ~1 attempt every 5s) that cannot be disabled.

### Quick start (local cluster)

```bash
# Builds the K8s images, renders a temporary local overlay with matching
# image tags, applies it, and waits for Postgres, RabbitMQ, and app rollouts.
cargo run -p xtask -- k8s deploy
```

The deploy waits up to 10 minutes for the pack-import Job to complete. Override
that when importing larger workflow packs:

```bash
cargo run -p xtask -- k8s deploy --pack-import-timeout-secs 900
```

The local overlay includes development-only Postgres, RabbitMQ, and app
Secrets. For k3d/kind clusters that do not share Docker Desktop's image store,
configure a local registry and pass it as `--local-registry localhost:5000` (or
use `--image-repository` for any registry reachable by the cluster).

Re-running `k8s deploy` against a cluster that already has the stack up
preserves the existing `runinator-postgres` and `runinator-rabbitmq`
StatefulSets by default, so redeploys don't roll your data stores. Pass
`--recreate-infra` when you actually want those StatefulSets re-applied (e.g.
after editing their manifests):

```bash
cargo run -p xtask -- k8s deploy --recreate-infra
```

To redeploy only the web interface, rebuild and apply just the
`runinator-command-center-web` resources with:

```bash
cargo run -p xtask -- k8s deploy --command-center-only
```

By default only the command-center is reachable from outside the cluster (it
proxies `/api` and `/ws` to the web service). To additionally expose the web
service API/websocket directly and open a debugging-only NodePort to Postgres,
pass `--expose-direct-ingress`:

```bash
cargo run -p xtask -- k8s deploy --expose-direct-ingress
```

This injects the `deploy/k8s/components/direct-ingress` component at render time
(it is never wired into a base/overlay, so prod stays closed unless you opt in).
It adds a host-based ingress for the web service at `api.runinator.local` and a
`NodePort` Service reaching Postgres on `<node-ip>:30432`. Leave the flag off for
any environment where the database must not be externally reachable.

Tear the stack back down with `cargo run -p xtask -- k8s delete` (same
`--manifest`/`--kube-context`/`--command-center-only` flags apply).

### Production

Edit `deploy/k8s/overlays/prod/storage-class-patch.yaml` to set your cluster's
`storageClassName`, create the three Secrets from
`deploy/k8s/base/secrets.example.yaml`, then build, push, render, and apply the
prod overlay:

```bash
cargo run -p xtask -- k8s deploy \
  --manifest deploy/k8s/overlays/prod \
  --kube-context my-prod-context \
  --image-repository registry.example.com/runinator \
  --image-tag 1.0.0
```

See `deploy/k8s/overlays/{local,prod}/README.md` for details.

Launch the Tauri command center against the deployed K8s stack with one
command. The script starts a local port-forward to the `runinator-ws` Service,
waits for the API, passes the forwarded service URL to the app, and stops the
forward when the UI exits:

```bash
bash scripts/run-k8s.sh ui
```

Use `--context` or `--namespace` when the stack is not in the current kubectl
context's `runinator` namespace:

```bash
bash scripts/run-k8s.sh ui --context my-prod-context --namespace runinator
```

To open the raw web-service API or Scalar docs directly in a browser, forward
the `runinator-ws` Service on a separate local port:

```bash
bash scripts/port-forward-ws.sh
```

That exposes:

- `http://127.0.0.1:8081/docs`
- `http://127.0.0.1:8081/openapi.json`

## Build Command Center

`runinator-command-center` is a Tauri client. Run it against the local stack with:

```bash
bash scripts/run-local.sh ui
```

The default local stack advertises and serves the API on `127.0.0.1:8080`.
For Kubernetes, gossip is disabled and the web service is available through the
`runinator-ws` Service instead. Use the K8s UI launcher to create the
port-forward and pass the concrete API URL:

```bash
bash scripts/run-k8s.sh ui
```

The command center checks `RUNINATOR_COMMAND_CENTER_SERVICE_URL`,
`RUNINATOR_SERVICE_URL`, then `WS_API_BASE_URL` before falling back to gossip.
It is a pure client and does not execute workflow actions itself; use the
desktop agent below to run actions on your own machine.

## Desktop Agent

`runinator-desktop-agent` is a standalone binary that runs a machine as a
sandboxed local-files worker, controlled through a small tray-icon GUI instead
of a terminal. It shares its runtime with `runinator-worker` (same action
loop), but only ever runs the local-files provider against a folder you pick,
registered as an exclusive `desktop`-pool replica so it never picks up general
workloads.

```bash
cargo run -p runinator-desktop-agent
```

The process starts hidden in the tray; click the tray icon (or its "Open"
menu item) to bring up the control window, fill in the service URL, broker
URL, and sandbox folder, then start the agent. Closing the window just hides
it again — use "Exit" from the tray menu to actually quit.

## Package macOS Backend Apps

The Rust backend services remain normal command-line binaries. On macOS, you can
also package the runtime services as `.app` bundles with the Runinator icon:

```bash
cargo install cargo-packager --version 0.11.8 --locked
scripts/package-macos-backend-apps.sh --release
```

The script creates `.app` bundles for broker, web service, waker, worker,
the control CLI (`runinatorctl`), and supervisor under `target/macos-apps`.

## Verification

For workflow pack import changes, run:

```bash
jq empty packs/sdlc/sdlc.wdlp
jq empty packs/hello-world/hello-world.wdlp
cargo test -p runinator-ctl
```

To sync the seed file manually against a running local API:

```bash
bash scripts/run-local.sh sync
```

To run the tiny smoke pack against a running local stack:

```bash
bash scripts/run-local.sh smoke-sync
```

To verify rich workflow execution end-to-end against an isolated local stack:

```bash
RUNINATOR_E2E=1 cargo test -p runinator-e2e -- --ignored
```
