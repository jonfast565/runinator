# AGENTS.md

Guidance for agents working in this repository. Keep changes aligned with the existing architecture before adding new abstractions or cross-crate dependencies.

## Project Shape

Runinator is a Rust workspace for scheduling and executing tasks across a small distributed runtime using a resumable state-machine orchestrator.

Primary runtime flow:

1. `runinator-ws` owns the HTTP API and persists scheduled tasks, workflow runs, and orchestration records through `runinator-database`.
2. `runinator-scheduler` discovers the web service, fetches due state-machine nodes through `runinator-api`, and publishes task commands to `runinator-broker`.
3. `runinator-worker` polls the broker, resolves a provider/plugin, executes the task, and records results back through `runinator-api`.
4. `runinator-importer` imports task definitions and workflow packs into the web service.
5. `runinator-supervisor` runs the local stack from `runinator-supervisor.json`.

There is also a Tauri `runinator-command-center` client. Keep frontend UI changes separate from runtime crates unless the change explicitly touches the desktop UI.

## Crate Boundaries

Keep dependency direction boring and predictable, structured with domains in mind:

- `runinator-models`: shared domain and wire structs only. Avoid service logic, database details, HTTP clients, broker behavior, or runtime configuration here.
- `runinator-comm`: shared communication contracts and gossip/discovery types. It can depend on models, but should not know about concrete services, databases, providers, or broker backends.
- `runinator-api`: HTTP client facade for talking to the web service. Keep URL discovery behind locator types; do not spread raw web-service endpoint construction through scheduler, worker, or importer code.
- `runinator-database`: persistence interfaces and concrete SQLite/Postgres implementations. Database-specific mapping belongs here, not in `runinator-ws`.
- `runinator-ws`: API server and repository orchestration. It should depend on the database trait, not on worker/scheduler/provider internals.
- `runinator-broker`: broker trait, message/delivery types, in-memory backend, HTTP backend/client/server, and future broker adapters. Scheduler and worker should talk to the `Broker` trait where practical.
- `runinator-scheduler`: scheduling loop and state-machine node dispatch. It should not execute task providers directly and should not write to the database directly.
- `runinator-worker`: task execution loop and provider resolution. It should not calculate schedules or mutate state except through API calls intended for worker results.
- `runinator-workflows`: workflow validation, graph cycle detection, and condition evaluation logic.
- `runinator-plugin`: dynamic plugin loading and `Provider` trait integration. Keep FFI details contained here.
- `runinator-provider-*`: provider implementations. Always implement a new library for a new provider. Keep provider-specific configuration and external system behavior out of core crates.
- `runinator-utilities`: small cross-cutting helpers such as startup/logging, credential store trait, and data export. Do not turn this into a dumping ground for domain logic.

If a change requires a dependency from a lower-level/shared crate back into a service crate, stop and redesign the boundary.

## Coding Standards

- Favor guard clauses over deep nesting to keep logic flow flat and readable.
- If a functionality can have different implementations, always use traits to define the interface.
- Favor comments as appropriate for Rust but make them lower case, single line, with a period at the end.
- Use RustDoc comments (`///`) where necessary on public methods, but keep them short, succinct, dense, and dispassionate.
- Do not put all the code for a library in `lib.rs`; break it out into smaller, focused files.
- Do not put tests in the same files as code; break them out into a `tests.rs` file (or a `tests` module in a separate file).

## Runtime Contracts

Preserve the command lifecycle:

- Workflows are executed as state-machines with nodes like `task`, `wait`, `condition`, `approval`, `loop`, and `subflow`.
- Scheduler publishes `ActionCommand` values through `runinator-broker` for `task` nodes.
- Workflow run states (`queued`, `running`, `waiting`, etc.) are persisted separately from individual task run statuses.
- Workers acknowledge broker deliveries only after processing and any required result logging has completed.
- Worker outputs, logs, artifacts, and node-run status/results may be delivered as broker result events consumed by `runinator-ws`, or through compatibility endpoints in `runinator-api`; only `runinator-ws` persists them through `runinator-database`, and workers must not write directly to the database.
- Broker messages should remain serializable and backend-neutral.
- Any command or control payload that crosses the broker/scheduler/worker boundary must use the shared contracts in `runinator-comm` end to end. Do not add broker-local, scheduler-local, or worker-local duplicates for the same control path; extend `ActionCommand` or `ControlCommand`/`ControlKind` and thread that type through every relevant backend and delivery wrapper.
- Do not add direct scheduler-to-worker request/response channels. The only direct worker-to-scheduler path is the optional protobuf control-event ingress for lightweight lifecycle/control events. If a worker response needs to become durable or observable, put it on the existing `runinator-api` result/log/artifact path unless there is a documented reason it cannot use that path.
- Discovery/gossip types in `runinator-comm` should stay transport-friendly and serde-compatible.

When adding fields to shared structs, check every boundary that serializes, persists, or maps that type:

- `runinator-models`
- `runinator-comm`
- `runinator-database/src/mappers.rs`
- SQLite/Postgres implementations
- `runinator-api`
- importer task/pack JSON
- command center models, if the field is user-facing

## Provider And Plugin Guidance

Providers execute task actions; they are not schedulers, API clients, or persistence layers.

- Keep provider resolution in `runinator-worker`.
- Keep dynamic library loading and FFI safety wrappers in `runinator-plugin`.
- Treat plugin ABI names (`runinator_marker`, `name`, `call_service`) as public contracts.
- Provider/action metadata belongs next to the executable provider: built-ins expose it through `Provider::metadata()`, and plugins expose it through the `metadata` ABI function. Do not duplicate provider metadata in workflow or provider packs.
- For third-party integrations, look for a well-maintained client library before hand-rolling HTTP payloads and API semantics.
- Always add a new provider as a separate crate: `runinator-provider-<name>`.
- Keep `action_name`, `action_function`, and `action_configuration` semantics compatible with existing task import and execution paths.

## Database And API Guidance

The database crate owns persistence behavior. The web service owns HTTP behavior.

- Add new persistence operations to `DatabaseImpl` first, then implement them for SQLite and Postgres together.
- Keep SQLx row mapping centralized in `runinator-database`, especially `mappers.rs`.
- Keep repository functions in `runinator-ws/src/repository.rs` thin. They should orchestrate database calls and web responses, not duplicate SQL behavior.
- Keep public API payloads in shared model/API crates when they must be consumed by multiple binaries or the command center.

## Configuration

CLI/config changes usually affect more than one place.

Check these when adding or renaming runtime options:

- the crate's `config.rs` or `cli.rs`
- `runinator-supervisor.json`
- `README.md` and crate-specific README files
- `runinator-stack.yaml`
- Dockerfiles for service binaries
- local scripts such as `build.ps1`

Local development defaults should continue to work with:

```bash
cargo build --workspace
cargo run -p runinator-supervisor -- start
cargo run -p runinator-supervisor -- status
cargo run -p runinator-supervisor -- stop
```

## Error Handling And Async

- Prefer returning `SendableError` where the crate already uses that convention.
- Preserve structured `RuntimeError` codes where call sites already use them.
- Do not use `unwrap` or `expect` in runtime paths unless the process truly cannot continue and existing style already does so nearby.
- Keep blocking provider/plugin execution inside `spawn_blocking` or equivalent isolation.
- Preserve graceful shutdown with `Notify` and `ctrl_c` patterns in service binaries.
- Avoid holding locks across `.await`.

## Tests And Verification

Before handing off non-trivial Rust changes, run the narrowest useful checks first:

```bash
cargo fmt --all --check
cargo test -p <crate>
cargo test --workspace
```

Use `cargo check -p <crate>` when a full test run is slow or when the crate has no tests. If a change touches shared contracts, prefer `cargo test --workspace`.

For command center changes, use the existing Tauri build path and verify UI behavior separately.

## Change Hygiene

- Read nearby code before editing; mirror existing naming, async style, and error conventions.
- Keep edits scoped to the crate that owns the behavior.
- Do not introduce new workspace dependencies for small conveniences.
- Do not move shared structs between crates casually; that is a public boundary change.
- Avoid broad refactors while fixing localized behavior.
- Keep generated/runtime artifacts out of commits, especially `build/`, `target/`, and `.runinator-supervisor/`.
- Update docs/config examples in the same change when behavior changes.

## Architecture Checklist

Before adding code, ask:

- Does this crate own the behavior I am changing?
- Is the dependency direction still from services toward shared contracts, not the reverse?
- Are scheduler, worker, web service, broker, and database responsibilities still distinct?
- Have all serializers, mappers, API clients, and config files been updated for shared contract changes?
- Can the local supervisor stack still run after this change?
