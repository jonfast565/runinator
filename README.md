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

The supervisor starts the importer with a short local polling interval, so edits to `runinator-importer/tasks/tasks.json` are pushed into the API shortly after the web service is discovered. If the stack is already running and you want an immediate sync, run:

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

Runtime files are written under `.runinator-supervisor/`. Child process stdout and stderr are collected under `.runinator-supervisor/logs/` with one file per process start:

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

This publishes binaries and the seed file under `target/artifacts/`, writes `target/artifacts/runinator-supervisor.local.json`, then starts the stack in the foreground. Stop it with `Ctrl+C`.

## Seeded Mock SDLC Workflow

The importer reads `runinator-importer/tasks/tasks.json`. It seeds both scheduled tasks and workflow definitions, including:

- workflow `1001`: `Mock SDLC: Feature Delivery`
- mock console task IDs `101-106`

In `command-center`, open the Workflows tab, select `Mock SDLC: Feature Delivery`, and run it. The workflow advances through local console-backed SDLC steps, pauses for `review_approval`, continues after approval, then pauses again for `release_gate`. Use the generic Approvals view to approve those requests and let the workflow finish.

The mock task definitions are disabled as scheduled tasks, so they do not run from cron. They are still executable as workflow task nodes.

## Build Command-Center

`command-center` is a separate C++/Qt client. Build it with the existing CMake project:

```bash
cmake -S command-center -B command-center/build
cmake --build command-center/build
```

Then launch the generated app from `command-center/build` and connect to the local service. The default local stack advertises and serves the API on `127.0.0.1:8080`.

## Verification

For importer/workflow seed changes, run:

```bash
jq empty runinator-importer/tasks/tasks.json
cargo test -p runinator-importer
```

To sync the seed file manually against a running local API:

```bash
bash scripts/run-local.sh sync
```
