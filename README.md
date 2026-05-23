# runinator

Runinator is a Rust workspace for scheduling and executing tasks across a small local/distributed runtime. The local development path uses `runinator-supervisor` to run the broker, web service, scheduler, worker, and importer together.

## Prerequisites

- Rust toolchain with Cargo.
- PowerShell 7+ if using `build.ps1`.
- Qt 6 and CMake only if you want to build the C++ `command-center` app.

## Run Locally

The quickest path on macOS/Linux is:

```bash
bash scripts/run-local.sh start
```

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
bash scripts/run-local.sh stop
bash scripts/run-local.sh restart
```

The supervisor starts the importer with a short local polling interval, so edits to `runinator-importer/workflows/workflows.json` are pushed into the API shortly after the web service is discovered. If the stack is already running and you want an immediate sync, run:

```bash
bash scripts/run-local.sh sync
```

You can also run the supervisor directly:

```bash
cargo build --workspace
cargo run -p runinator-supervisor -- start
cargo run -p runinator-supervisor -- status
cargo run -p runinator-supervisor -- stop
```

This uses `runinator-supervisor.json` to start:

- `runinator-broker`
- `runinator-ws`
- `runinator-scheduler`
- `runinator-worker`
- `runinator-importer`

The default worker configuration processes up to four actions concurrently. Tune
`--max-concurrent-actions` when long-running actions should not block unrelated
workflow action pickup.

The local stack uses the built-in broker over raw TCP by default. The standalone
broker can also serve the same broker contract over HTTP by setting
`RUNINATOR_BROKER_TRANSPORT=http`; HTTP clients must use an endpoint like
`http://127.0.0.1:7070/`, while TCP clients use `127.0.0.1:7070`.

The optional direct worker-to-scheduler control-event channel is disabled by
default. Enable it on the scheduler with `--worker-control-transport http|tcp`
plus bind/port flags, and on workers with `--scheduler-control-transport
http|tcp` plus the matching endpoint.

Local runtime files are written under `~/.runinator/` by default. This includes
the SQLite database at `~/.runinator/runinator.db`, credentials at
`~/.runinator/credentials.enc.json`, application logs under
`~/.runinator/logs/`, and supervisor state under `~/.runinator/supervisor/`.
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

This publishes binaries and the workflow import file under `target/artifacts/`, writes `target/artifacts/runinator-supervisor.local.json`, then starts the stack in the foreground. Runtime state still goes under `~/.runinator/` unless you pass explicit paths. Stop it with `Ctrl+C`.

## Workflow Import

The importer reads `~/.runinator/workflows/workflow-pack.json` by default. The
supervisor config in this repository points it at `packs/sdlc/workflow-pack.json`
for local development. Put a secret bundle at `~/.runinator/secrets.json` to
load local credentials during importer startup.

Workflow syntax now includes richer declarative control-flow nodes:

- `switch` routes by ordered cases and an optional default target.
- `parallel` starts branch roots, with branch nodes returning to a `join`.
- `join` waits for named upstream nodes using `all`, `any`, or `first_success`.
- `try` runs a body, optional catch, and optional finally node; those nodes transition back to the `try` controller.
- `map` runs one target node for each resolved item and exposes the current item under `workflow.state.map`.
- `race` starts branch roots until one satisfies the winner policy; v1 does not cancel already dispatched work.
- `emit` records structured node output without calling a provider.
- `reentry` allows explicit bounded cycles back to a node and can route to `on_exhausted`.

The v1 control-flow runtime is controller-driven and still uses one `active_node_id`.
`parallel` and `race` advance branch roots sequentially through persisted workflow state,
and `map.concurrency` is reserved for a future multi-active-node runtime. Branch/body/item
nodes should transition back to their owning `join`, `try`, `map`, or `race` controller.

## Build Command-Center

`command-center` is a separate C++/Qt client. Build it with the existing CMake project:

```bash
cmake -S command-center -B command-center/build
cmake --build command-center/build
```

Then launch the generated app from `command-center/build` and connect to the local service. The default local stack advertises and serves the API on `127.0.0.1:8080`.

## Verification

For importer workflow import changes, run:

```bash
jq empty runinator-importer/workflows/workflows.json
cargo test -p runinator-importer
```

To sync the seed file manually against a running local API:

```bash
bash scripts/run-local.sh sync
```

To verify rich workflow execution end-to-end against an isolated local stack:

```bash
RUNINATOR_E2E=1 cargo test -p runinator-e2e -- --ignored
```
