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

The supervisor runs `runinatorctl workflows apply` once, so the workflow pack configured in `runinator-supervisor.json` is pushed into the API after the web service starts. The checked-in local config imports the WDL pack manifest at `packs/sdlc/sdlc.wdlp`, which compiles the referenced `.wdl` files before sending the bundle to the API. (Built-in provider metadata is registered separately: the worker self-publishes it to the web service on startup.) If the stack is already running and you want another sync, run:

```bash
bash scripts/run-local.sh sync
```

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

WDL references resolve runtime values into action arguments. Alongside `input.*`,
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

Schema is applied by the `runinator-migration` image, which uses sqlx's built-in
migrator to run versioned SQL files from `runinator-database/migrations/`. The
`runinator-ws` Deployment runs migrations from an initContainer on every pod
start. `deploy/k8s/base/db-migrate-job.yaml` is kept as an optional
out-of-band ops manifest; it is not part of the default kustomize base because
Kubernetes Job pod templates are immutable across image tag changes.

For non-Kubernetes environments, `runinator-migration` also supports
`--database mysql` / `--database mariadb` with a `mysql://...` connection string,
in addition to the existing SQLite and Postgres modes.

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
