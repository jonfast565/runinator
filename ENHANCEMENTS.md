# Runinator Enhancement Roadmap

## Context

This is an advisory survey, not a single implementation task. Based on a survey of the workspace (runtime crates, broker, database, auth, and the Tauri/Vue command center), the codebase is architecturally sound and feature-rich, but has clear gaps in **operational maturity**, **frontend polish/accessibility**, and **runtime/language completeness**. Below is a prioritized roadmap; each item names the owning crate(s) and key files so any one can be turned into a focused implementation plan later.

The guiding constraint from `AGENTS.md`: keep dependency direction services→shared-contracts, keep changes scoped to the crate that owns the behavior, and thread any shared-contract change through every broker backend, mapper, and config file.

> **Note:** the original Tiers 1–2 (operational hardening — tracing/metrics, DLQ/audit, retry backoff+jitter, rate limiting — and runtime/language completeness — poll/while, race-branch cancellation, plugin FFI cancellation, authorization phase 2) are all implemented and have been retired from this roadmap. The tiers below are the remaining work, renumbered.

---

## Tier 1 — Command center UX & accessibility

The desktop client is feature-complete but light on polish; these are user-facing wins.

### 1.1 Dark mode
- Light theme only; hard-coded color tokens in `src/styles/base.css` (`--surface: #ffffff`, etc.). Introduce CSS theme variables + `prefers-color-scheme` + a toggle. Mechanical but broad.

### 1.2 Accessibility pass
- ~29 ARIA attributes across 62 components. Add `aria-label`/`title` to icon buttons, focus trapping in modals (`WorkflowStepEditorModal.vue`), text fallback for color-only status badges, and a semantic heading hierarchy.

### 1.3 Live expression preview
- `ExpressionJsonEditor.vue` detects unresolved references but cannot evaluate expressions against sample data. Per memory `project_cc_expression_editor`, a TS-only evaluator/live-preview isn't possible today. Option: a ws "evaluate expression" endpoint the editor calls for preview — highest-value editor improvement.

### 1.4 Bulk actions, loading/empty states, error recovery
- No multi-select/bulk enable-disable-delete-rerun across workflows/runs. Add skeletons/spinners (only "Loading logs…" exists), richer empty states, and a "Retry" affordance on the error toast (`ToastHost.vue`) instead of console-only errors.

---

## Tier 2 — Test coverage & robustness

### 2.1 Backend test gaps
- Zero tests: `runinator-waker`, `runinator-supervisor`, `runinator-bootstrap`, and providers `aws`/`catalog`/`console`/`sql`. The waker (core timer/relay loop) being untested is the notable risk.
- Add DB round-trip/migration tests in `runinator-database` (none currently exercise sqlite↔postgres schema parity).

### 2.2 Frontend test gaps
- 0% component test coverage for the entire `components/workflow/` directory (canvas, node, step editor — the most complex, highest-LOC components). Utilities/stores are well covered (~80%); components are ~5%.

### 2.3 Panic hardening
- `expect()` clusters in `runinator-wdl/src/parser.rs:92-132` (parser state) and `runinator-ws/src/openapi.rs` (11 calls). Convert runtime-path panics to structured `RuntimeError`s per the error-dictionary convention.

---

## Tier 3 — Remaining production gaps (2026-06-29 survey)

Tier 1 operational hardening is largely done (retry/backoff/jitter, executor lease, DLQ/audit, tracing+`trace_id`, `/metrics`, rate limiting, `/health`+`/ready`, graceful shutdown, per-node cancellation). These are the gaps that remain before leaning on the runtime in production.

### 3.1 Waker has no test coverage — highest residual risk
- The waker is the timer/relay heartbeat of the whole system: if it stalls, nothing fires. It currently has zero tests (also noted in 2.1). Add an integration test for the `wake → ingress → drive` path and an alert/metric on wake-channel lag before relying on it in prod.

### 3.2 Slow failover on a dead worker
- `EXECUTOR_LEASE_GRACE_SECONDS = 60` (`runinator-worker/src/main.rs`) means a crashed worker's node run is not reclaimable until `timeout_seconds + 60s` elapses. With long job timeouts, a pod crash strands that node for the full timeout window. Consider invalidating the lease off the worker replica heartbeat (already tracked via `register_replica_session`/`spawn_replica_heartbeat`) instead of only the action deadline.

### 3.3 Panic hardening (carryover from 2.3)
- `expect()` clusters in `runinator-wdl/src/parser.rs:92-132` and `runinator-ws/src/openapi.rs` (11 calls). A malformed pack or request should not be able to panic a handler. Convert runtime-path panics to structured `RuntimeError`s per the error-dictionary convention.

### 3.4 DB migration parity tests
- No tests exercise sqlite↔postgres (↔mysql) schema parity (carryover from 2.1). Schema drift between backends is a classic production surprise; add round-trip/migration parity tests in `runinator-database`.

---

## Tier 4 — Worker / job authoring pitfalls

These are footguns when creating new providers and workflow jobs, grounded in `runinator-worker/src/executor.rs` and `main.rs`. Worth capturing in a provider-authoring checklist so new jobs inherit the right defaults.

### 4.1 Make every provider action idempotent (the big one)
- The executor lease (`claim_workflow_node_run_executor`) prevents *concurrent* duplicate execution, but it **fail-opens on a transport error** (`main.rs:513-517`) and only protects while held. A worker that crashes *after* a side effect but *before* `broker.ack` will redeliver and re-execute. Any action with external side effects (charges, posts, writes) must dedupe on its own key — `workflow_node_run_id` is available in the request and is a natural idempotency key.

### 4.2 A timeout stops *waiting*, not the work
- Provider code runs in `spawn_blocking` (`executor.rs:69`). On timeout the `CancellationToken` is cancelled, but a provider that never polls the token (or has no internal client timeout) keeps running on a blocking thread after the node is already marked `TimedOut`. Consequences: (a) Tokio blocking-pool thread leak (default 512 — exhaust it and the worker wedges), and (b) a "timed out" job still mutating the outside world. **Rule for new providers:** honor the cancellation token in any loop, and set an explicit client timeout ≤ `request.timeout_secs`.

### 4.3 Don't model "wait for X" as a long-running task
- Each in-flight action pins one blocking thread *and* one concurrency permit for its whole duration. A task that sleeps/polls for an hour burns both the entire time. Use the `wait` / `gate` / `signal` node kinds, which park in the reducer with zero worker footprint. Tasks should be short, active work.

### 4.4 Tune `max_concurrent_actions` per workload
- It is a single per-worker semaphore across *all* action types (`main.rs:255`). One memory-heavy job × high concurrency can OOM the pod and starve light jobs queued behind it. For heterogeneous workloads, run separate worker deployments tuned per workload rather than one large pool.

### 4.5 Consumer-group default differs by backend (horizontal-scaling gotcha)
- `broker_consumer_id` defaults to the shared group `runinator-workers` on **kafka**, but to a fresh per-worker `worker_id` UUID on **rabbitmq/http/tcp/in-memory** (`config.rs:90`). Whether N workers *compete* for actions or each receives *every* action depends on the backend's consumer-id→group mapping. **When scaling workers on a non-kafka backend, set `broker_consumer_id` explicitly to the same value across the fleet** so they compete instead of double-executing. Verify on the chosen backend before scaling past one worker.

### 4.6 Secret resolution is on the job's critical path
- `resolve_secret_refs` runs per delivery (`main.rs:532`). If the settings store is unavailable, the job publishes `Failed` and acks — it does *not* retry at the broker level. Jobs touching `secret://` refs should carry a node-level `retry` policy so a transient secret-store blip recovers.

### 4.7 Result-publish failures redeliver the whole action
- If a job succeeds but `publish_status`/`flush` fails (`main.rs:636,680`), the delivery is nacked and the entire action re-runs — looping back to 4.1. Idempotency (4.1) is the mitigation here too.

---

## Tier 5 — Net-new product capabilities (2026-07-12 survey)

These are **new product features** rather than hardening of existing ones — capabilities the runtime does not have today. Each was confirmed against the current codebase as a genuine gap. Ordered by leverage; #5.3 (webhook triggers) is now the highest-value remaining pick.

### 5.1 Workflow test harness + dry-run simulation — ✅ implemented
- **Owning crates:** `runinator-workflows`, `runinator-engine`, `runinator-ctl`, `runinator-ws`, `runinator-api`, `runinator-command-center`.
- **What shipped:** A `SimulationEnv` evaluator interface in `runinator-workflows` (`simulate.rs`) with two implementations — the `MockEnv` test impl (`testkit.rs`, driven by a `.wdlt` spec) and a `DbSimulationEnv` db impl in `runinator-engine` (`simulate.rs`, config from the settings store + a prior run's recorded outputs). `simulate_workflow` walks the state machine reusing the reducer's own `next_transition`/`evaluate_switch`/`evaluate_toggle`/`evaluate_percentage`/condition evaluators, stubs task/park nodes through the interface, and publishes no `ActionCommand`s. `.wdlt` suites (JSON) assert on status, reached/not_reached nodes, router branch targets, and final outputs; `runinatorctl workflows test <pack>` runs them offline and exits non-zero on failure. Fan-out kinds (loop/parallel/join/map/race/try/subflow) are reported as unsupported rather than simulated incorrectly.
- **Server-side dry-run/branch-preview (follow-on):** `POST /workflows/simulate` (`WorkflowSimulateRequest`) drives the `DbSimulationEnv` against live config and, optionally, a prior run's outputs (`replay_run`) — no actions published. Authz: a saved workflow requires `Run`; an unsaved draft only an authenticated caller; `replay_run` is additionally gated on that run's workflow. Exposed through the async/blocking `runinator-api` clients (`simulate_workflow`) and the command center: a **Dry run** button in the workflow toolbar opens a modal that walks the current draft and shows the routed path, per-node status, branch targets, and final output.
- **Boundary note (honored):** dry-run lives in the `runinator-workflows` evaluation path; the db impl lives one layer up in `runinator-engine`; the endpoint is a thin `runinator-ws` handler; nothing touches the broker.

### 5.2 AI-assisted WDL authoring in the command center
- **Owning crates:** `runinator-command-center`, `runinator-provider-ai`.
- **Problem:** Authoring WDL/graphs is manual; new users face a blank canvas.
- **Approach:** Natural-language → WDL draft, generated against the live backend-driven node/edge/trigger **catalog metadata** (per memory `project_catalog_metadata_reactivity`). "Add a Slack notify after the approval fails" edits the draft graph in place. The catalog gives the model a constrained, validated tool surface so it emits well-formed graphs rather than free text. Draft stays the source of truth (per `project_cc_workflow_editing`).

### 5.3 Inbound webhook *triggers* (start a run)
- **Owning crates:** `runinator-ws` (`handlers/webhook.rs`, trigger materialization), `runinator-models` (triggers).
- **Problem:** `handlers/webhook.rs` only *wakes/signals an already-parked run*; there is no way to **start** a new run from an inbound event. Triggers are cron + chained today (`metadata.triggers`).
- **Approach:** Add a `trigger webhook "..."` header declaration that mints a signed inbound URL to start a new run, with a payload-mapping expression into workflow inputs. Reuse the existing pack-managed-trigger materialization path (`metadata.managed_by = "wdl"`).
- **Boundary note:** the trigger kind is a shared-contract change — thread through `runinator-models` triggers, ctl WDL compile, mappers, and the command-center trigger catalog.

### 5.4 Backfill + freeze/blackout windows
- **Owning crate:** `runinator-ws` (trigger-firing loop), `runinator-ctl`.
- **Problem:** No way to replay missed cron slots, and no way to suppress firing during a change freeze or holiday.
- **Approach:** Backfill — `runinatorctl workflows backfill <wf> --from --to` synthesizes trigger firings for past/missed cron slots. Freeze windows — a calendar (change-freeze, holidays) the trigger-firing loop consults to defer firing until the window closes. Both localize to the trigger-firing loop.

### 5.5 Run timeline / Gantt visualization — ✅ implemented
- **Owning crate:** `runinator-command-center`.
- **What shipped:** A proportional Gantt timeline in the run detail. Pure layout logic lives in `core/workflow/run-gantt.ts` (`buildGanttLayout`, unit-tested) and a thin `ui/components/shared/RunGantt.vue` renders it: one bar per node run positioned on a shared time axis, a dashed segment for queued/parked wait before a node goes active, retry (`attempt`) badges, and the longest active segment highlighted as the critical-path bottleneck. Live bars count up while a run is in flight. Rendered entirely from the `started_at`/`finished_at`/`attempt` fields already persisted — no backend change. Complements the existing vertical `RunTimeline` (step list) with a duration-proportional view.

### 5.6 AI cost & token accounting
- **Owning crates:** `runinator-provider-ai`, `runinator-models` (result event), `runinator-database`.
- **Problem:** `provider-ai` (claude_code) captures **no** token/cost usage. There is no hook to attribute AI spend per node/run/workflow.
- **Approach:** Capture usage in the provider, thread it back on the `WorkflowResultEvent`, persist per node-run, and roll up per run/workflow in the command center.
- **Boundary note:** adding usage to the result event is a `runinator-comm`/`runinator-models` contract change — thread through every broker backend, `mappers.rs`, and both DB backends.

### 5.7 Pack environments + promotion
- **Owning crates:** `runinator-ctl`, `runinator-ws` (packs), settings store.
- **Problem:** `semver.rs` exists but there is no dev→staging→prod lifecycle; a pack imports with one fixed set of config/secret bindings.
- **Approach:** Environment-scoped pack deployment with a diff/promote flow (`runinatorctl workflows promote <pack> staging→prod`) and per-environment config/secret binding, so the same compiled pack runs against different settings-store values per environment.

---

### Recommended sequencing

1. ~~**5.1 (test harness + server-side dry-run/branch-preview)**~~ — ✅ done; `POST /workflows/simulate` now backs the command center's Dry run modal and feeds 1.3 live expression preview and 5.2 AI authoring.
2. **5.3 (webhook triggers)** — highest reach; reuses the pack-managed-trigger path. Workflow-to-workflow chaining already shipped, so **5.4 (backfill/freeze)** is a natural follow-on once trigger kinds keep extending.
3. ~~**5.5 (run timeline)**~~ — ✅ done.
4. **5.6 (AI cost)** and **5.2 (AI authoring)** as the AI surface grows.
5. **5.7 (environments)** once multi-env deployment is a real need.
6. **1.1 / 1.2 (dark mode + a11y)** can run in parallel as low-risk UX wins throughout.

---

## Verification (per area, when implemented)

- **Backend:** `cargo fmt --all --check`, `cargo test -p <crate>`, then `cargo test --workspace` for shared-contract changes. Confirm the local stack still runs: `cargo run -p runinator-supervisor -- start|status|stop`.
- **WDL changes:** round-trip a `.wdl` through compile→decompile→format and confirm idempotency.
- **Frontend:** existing Vitest path (`*.test.ts`) plus the Tauri build path; verify dark mode toggle and keyboard/focus behavior manually.

---

## Note

This roadmap is a survey for prioritization — no single item is fully specified for execution yet. Pick one (e.g. "do 5.1") to get a detailed, file-by-file implementation plan.
