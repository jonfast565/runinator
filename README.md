# runinator

Runinator is a Rust workspace for scheduling and executing tasks across a small local/distributed runtime. The local development path uses `runinator-supervisor` to run the broker, web service, waker, and worker, plus a one-shot `runinatorctl` pack import.

## Prerequisites

- Rust toolchain with Cargo.
- PowerShell 7+ if using `build.ps1`.
- kubectl if deploying to Kubernetes or launching the UI against a K8s stack.
- pnpm if you want to build or run the Tauri `runinator-command-center` app.

## Run Locally

The quickest path on macOS/Linux is:

```bash
bash scripts/run-local.sh start
```

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
bash scripts/run-local.sh status
bash scripts/run-local.sh watch
bash scripts/run-local.sh logs
bash scripts/run-local.sh logs --process web-service
bash scripts/run-local.sh logs-watch --lines 40
bash scripts/run-local.sh sync
bash scripts/run-local.sh dev
bash scripts/run-local.sh smoke-sync
bash scripts/run-local.sh ui
bash scripts/run-local.sh stop
bash scripts/run-local.sh restart
```

The supervisor runs `runinatorctl workflows apply` once, so the workflow pack configured in `runinator-supervisor.json` is pushed into the API after the web service starts. The checked-in local config imports the WDL pack manifest at `packs/sdlc/sdlc.wdlp`, which compiles the referenced `.wdl` files before sending the bundle to the API. It also advertises `127.0.0.1` for the web service, waker, and worker, and gives the waker and worker stable local instance ids so the replicas list shows host/IP/version data instead of blank fields on restart. Built-in provider metadata is seeded by the web service from the provider catalog on startup. If the stack is already running and you want another sync, run:

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

Worker and waker nodes can be spun up and scaled down on demand through the web
service's pluggable provisioner. Two backends are available: `supervisor` (adds
dynamic local processes through the running `runinator-supervisor` control queue)
and `kubernetes` (scales the worker/waker Deployments via kube-rs; the ws image
must be built with `--features kubernetes` and the `runinator-ws-provisioner`
RBAC role applied). Enable a backend with `RUNINATOR_PROVISIONER_SUPERVISOR_ENABLED`
or `RUNINATOR_PROVISIONER_K8S_ENABLED`. The supervisor backend reads its spawn
templates from `RUNINATOR_PROVISIONER_SUPERVISOR_WORKER` /
`..._WAKER` (JSON `{ "command", "args", "env", "cwd" }`).

Drive it from the CLI or the command center's Node Pools panel (Replicas view):

```bash
runinatorctl nodes list
runinatorctl nodes spin-up --backend supervisor --kind worker --count 2
runinatorctl nodes scale --backend kubernetes --kind worker --desired 5
runinatorctl nodes stop --backend supervisor --node prov-worker-<id>
```

The web service owns the reducer and drives workflows over the broker: it
publishes scheduled work on the `wake` channel, and the `runinator-waker` (a
small, broker-only timer/relay) sleeps until each ready node is due and then
publishes a `drive` on the `ingress` channel that the web service consumes to
advance the run. The waker holds no state and reaches the web service only over
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

## PowerShell Local Run

PowerShell can build and run a local artifact layout:

```powershell
./build.ps1 -Mode Local -Run
```

This publishes binaries under `target/artifacts/`, writes `target/artifacts/runinator-supervisor.local.json`, then starts the stack in the foreground. Runtime state still goes under `~/.runinator/` by default. The default workflow import is `packs/sdlc/sdlc.wdlp`, and any referenced `.wdl` files are copied into `target/artifacts/workflows/` with the manifest. Stop it with `Ctrl+C`.

That generated local supervisor config now uses the same bootstrap-admin and
bootstrap service API-key flow as the checked-in shell path, so turning on
`RUNINATOR_AUTH_ENABLED=true` does not require hand-editing the artifact config.

To run that same local artifact flow against MariaDB, select the backend and
pass a MySQL-compatible URL:

```powershell
./build.ps1 -Mode Local -Run `
  -LocalDatabaseBackend mariadb `
  -LocalDatabaseUrl 'mysql://runinator:runinator@127.0.0.1:3306/runinator'
```

`-LocalDatabaseBackend postgres` works the same way with a Postgres URL. SQLite
continues to use `-LocalDatabasePath`.

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
- `race` starts branch roots until one satisfies the winner policy; v1 does not cancel already dispatched work.
- `emit` records structured node output without calling a provider.
- `reentry` allows explicit bounded cycles back to a node and can route to `on_exhausted`.

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

```bash
# point all binaries at a local OpenTelemetry Collector (OTLP/HTTP on :4318)
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
cargo run -p runinator-supervisor -- start
```

**In Kubernetes**, the `components/observability` kustomize component deploys an
OpenTelemetry Collector, Jaeger (trace UI), Prometheus (scrapes the collector), and
Grafana (dashboards over Prometheus + Jaeger), and points the services at the
collector. It is enabled in the `local` overlay by default; add
`../../components/observability` to another overlay's `components:` list to turn it
on there (and remove it to turn otel back off). After deploying:

```bash
# dashboards — open Grafana at http://localhost:3000 (anonymous admin; "Runinator
# Overview" dashboard is provisioned, with Prometheus + Jaeger datasources wired up)
kubectl -n runinator port-forward svc/runinator-grafana 3000:3000
# traces — open the Jaeger UI at http://localhost:16686
kubectl -n runinator port-forward svc/runinator-jaeger 16686:16686
# raw metrics — the Prometheus UI / API at http://localhost:9090
kubectl -n runinator port-forward svc/runinator-prometheus 9090:9090
# logs (and a copy of every signal) — the collector's debug exporter
kubectl -n runinator logs deploy/runinator-otel-collector
```

Grafana's anonymous-admin login is for convenient local viewing; lock it down (set
a real admin password and disable anonymous access) before using it on a shared
cluster.

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
# Builds the five K8s images, renders a temporary local overlay with matching
# image tags, applies it, and waits for Postgres, RabbitMQ, and app rollouts.
pwsh ./build.ps1 -DeployKube
```

The deploy waits up to 10 minutes for the pack-import Job to complete. Override
that when importing larger workflow packs:

```bash
pwsh ./build.ps1 -DeployKube -KubePackImportTimeoutSeconds 900
```

The local overlay includes development-only Postgres, RabbitMQ, and app
Secrets. For k3d/kind clusters that do not share Docker Desktop's image store,
configure a local registry and pass it as `-LocalRegistry localhost:5000` (or
use `-ImageRepository` for any registry reachable by the cluster).

To redeploy only the web interface, rebuild and apply just the
`runinator-command-center-web` resources with:

```bash
pwsh ./build.ps1 -DeployKube -CommandCenterOnly
```

By default only the command-center is reachable from outside the cluster (it
proxies `/api` and `/ws` to the web service). To additionally expose the web
service API/websocket directly and open a debugging-only NodePort to Postgres,
pass `-KubeExposeDirectIngress`:

```bash
pwsh ./build.ps1 -DeployKube -KubeExposeDirectIngress
```

This injects the `deploy/k8s/components/direct-ingress` component at render time
(it is never wired into a base/overlay, so prod stays closed unless you opt in).
It adds a host-based ingress for the web service at `api.runinator.local` and a
`NodePort` Service reaching Postgres on `<node-ip>:30432`. Leave the flag off for
any environment where the database must not be externally reachable.

### Production

Edit `deploy/k8s/overlays/prod/storage-class-patch.yaml` to set your cluster's
`storageClassName`, create the three Secrets from
`deploy/k8s/base/secrets.example.yaml`, then build, push, render, and apply the
prod overlay:

```bash
pwsh ./build.ps1 -DeployKube \
  -KubeManifest deploy/k8s/overlays/prod \
  -KubeContext my-prod-context \
  -ImageRepository registry.example.com/runinator \
  -ImageTag 1.0.0
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
