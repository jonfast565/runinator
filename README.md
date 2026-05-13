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
- workflow `1003`: `SDLC: Implement, Review, QA Until Done`
- workflow `1002`: `Rich Workflow Syntax Demo`
- mock console task IDs `101-106`
- real SDLC provider task IDs `201-208`

In `command-center`, open the Workflows tab, select `Mock SDLC: Feature Delivery`, and run it. The workflow advances through local console-backed SDLC steps, pauses for `review_approval`, continues after approval, then pauses again for `release_gate`. Use the generic Approvals view to approve those requests and let the workflow finish.

The real-provider SDLC workflow uses Jira, git, AI command, GitHub, `map`, wait, and approval nodes to process matching Jira issues into generated PRs, merge approved pull requests, and transition completed items. It requires runtime input for Jira credentials/query, repo/workspace paths, the implementation command, GitHub credentials, and the done transition ID.

The SDLC task definitions are disabled as scheduled tasks, so they do not run from cron. They are still executable as workflow task nodes.

Workflow syntax now includes richer declarative control-flow nodes:

- `switch` routes by ordered cases and an optional default target.
- `parallel` starts branch roots, with branch nodes returning to a `join`.
- `join` waits for named upstream nodes using `all`, `any`, or `first_success`.
- `try` runs a body, optional catch, and optional finally node; those nodes transition back to the `try` controller.
- `map` runs one target node for each resolved item and exposes the current item under `workflow.state.map`.
- `race` starts branch roots until one satisfies the winner policy; v1 does not cancel already dispatched work.
- `emit` records structured node output without calling a provider.

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

For importer/workflow seed changes, run:

```bash
jq empty runinator-importer/tasks/tasks.json
cargo test -p runinator-importer
```

To sync the seed file manually against a running local API:

```bash
bash scripts/run-local.sh sync
```

To verify the rich workflow demo end-to-end against an isolated local stack:

```bash
RUNINATOR_E2E=1 cargo test -p runinator-e2e -- --ignored
```
