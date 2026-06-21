# Runinator Enhancement Roadmap

## Context

This is an advisory survey, not a single implementation task. Based on a survey of the workspace (runtime crates, broker, database, auth, and the Tauri/Vue command center), the codebase is architecturally sound and feature-rich, but has clear gaps in **operational maturity**, **frontend polish/accessibility**, and **runtime/language completeness**. Below is a prioritized roadmap; each item names the owning crate(s) and key files so any one can be turned into a focused implementation plan later.

The guiding constraint from `AGENTS.md`: keep dependency direction services→shared-contracts, keep changes scoped to the crate that owns the behavior, and thread any shared-contract change through every broker backend, mapper, and config file.

---

## Tier 1 — Highest leverage (operational hardening)

These make the distributed runtime debuggable and safe to run in production. Today there is no way to correlate a workflow run across ws → broker → worker.

### 1.1 Distributed tracing + metrics — ✅ implemented
- **Status:** `runinator-utilities/src/logger.rs` now installs a `tracing-subscriber` registry (stdout + file layers, `RUNINATOR_LOG` `EnvFilter`, `log` macros bridged). A `trace_id` correlation id is carried on the `ActionCommand` / `WorkflowResultEvent` envelopes in `runinator-comm` (serde-default, so it flows through every broker backend) and the worker enters an `execute_action` span (`trace_id`/`run_id`/`node_id`/`attempt`) over each delivery. `runinator-ws` exposes a Prometheus `/metrics` endpoint (public, in OpenAPI) and the `stability.rs` counters (applied/duplicate/retried/dead-lettered/receive-errors) are promoted to `metrics` counters. Remaining/optional: OpenTelemetry OTLP export and W3C traceparent interop.
- **Problem:** No `tracing`/OpenTelemetry and no metrics export anywhere (grep confirms zero). Logging is plain-text `fern`/`log` only. Multi-process issues are effectively undebuggable.
- **Approach:** Adopt the `tracing` ecosystem in `runinator-utilities/src/logger.rs` (swap/augment fern with `tracing-subscriber`), propagate a `trace_id`/`run_id` span context across the broker boundary by adding it to the shared `runinator-comm` command envelopes (so it survives ws→worker hops). Add a Prometheus `/metrics` endpoint in `runinator-ws`. Promote the existing in-memory counters in `runinator-ws/src/stability.rs` (applied/duplicate/retried/dead-lettered) to real metrics.
- **Boundary note:** trace context on broker messages is a `runinator-comm` contract change — thread through every broker backend.

### 1.2 Persisted dead-letter queue + audit log — ✅ implemented
- **Problem:** Dead-lettered result/ingress events were logged and ack'd into the void (`runinator-ws/src/result_consumer.rs`, `background.rs`); there was no durable record of failed messages and no audit trail for auth/sensitive ops.
- **Done:** Added `dead_letters` and `audit_log` tables (sqlite/postgres/mysql migrations `20260620000001_dlq_audit.sql`), `mappers::row_to_dead_letter`/`row_to_audit_log`, and `DatabaseImpl` methods `record_dead_letter`/`fetch_dead_letters`/`record_audit_log`/`fetch_audit_log`. The result and ingress consumers persist a dead-letter record before acking the poison message (`crate::audit::persist_dead_letter`). Login success/failure and authz denials (`require_workflow`) are recorded via `crate::audit::record_audit`. Admin-only `GET /dead_letters` and `GET /audit_log` (in OpenAPI) expose them, with a command-center **Dead Letters** and **Audit Log** view (admin-gated nav, HTTP + Tauri runtimes).

### 1.3 Retry backoff + provider resilience — ✅ implemented
- **Problem:** Fixed 250ms retry backoff with no jitter (`result_consumer.rs`). No external-HTTP timeout on some providers (reqwest defaults).
- **Done:** `ResultConsumerPolicy` now computes exponential backoff with full jitter capped at 30s (`backoff_for`). Provider HTTP clients all set explicit timeouts; the one default client (`runinator-provider-email` notification post) now builds a 30s-timeout client. (Most providers already bound requests by `request.timeout_secs`.)

### 1.4 Rate limiting — ✅ implemented
- **Problem:** No rate limiting on the HTTP API (`runinator-ws/src/auth.rs` gates auth but not volume) — DoS/abuse risk once exposed.
- **Done:** Added `runinator-ws/src/rate_limit.rs`, a tower middleware token bucket keyed by the resolved principal (falling back to the connection IP), layered inside auth so it sees `AuthContext`. Configured via `RUNINATOR_RATE_LIMIT_ENABLED`/`_RPS`/`_BURST` (off by default); `/health`, `/ready`, `/metrics` exempt; over-limit returns `429` + `Retry-After`.

---

## Tier 2 — Runtime & language completeness

These remove known footguns and "reserved but not implemented" features.

### 2.1 `poll`/`while` loop construct (WDL + reducer) — ✅ implemented

### 2.2 Race-branch cancellation — ✅ implemented

### 2.3 Plugin FFI cancellation — ✅ implemented

### 2.4 Authorization phase 2 — ✅ implemented
- **Problem:** Resource-based authz was scaffolding only (`runinator-ws/src/authz.rs`, `runinator-models/src/auth.rs`); only workflow-level Own/Edit/Run/View with admin bypass, no enforcement coverage.
- **Done:** Grant enforcement is wired across every workflow/run-scoped endpoint — workflows, runs, node-runs, triggers, gates/approvals/automation records, artifacts, debug — via `require_workflow`/`require_run_workflow`/`require_*_workflow`, with `visible_workflow_ids`/`filter_records` scoping list responses and `grant_owner` stamping creators. Runs and sub-resources inherit the parent workflow's permission (no separate ResourceType yet — by design). Api-key minting is inline-scoped (only admins mint service/cross-user keys). The command-center `PermissionsView` (Users/Teams/Access/API-keys) is fully wired and admin-gated via `visibleNavSections()`. Enforcement is covered by `workflow_permission`/`require_workflow`/`visible_workflow_ids` tests in `runinator-ws/src/tests.rs`.

---

## Tier 3 — Command center UX & accessibility

The desktop client is feature-complete but light on polish; these are user-facing wins.

### 3.1 Dark mode
- Light theme only; hard-coded color tokens in `src/styles/base.css` (`--surface: #ffffff`, etc.). Introduce CSS theme variables + `prefers-color-scheme` + a toggle. Mechanical but broad.

### 3.2 Accessibility pass
- ~29 ARIA attributes across 62 components. Add `aria-label`/`title` to icon buttons, focus trapping in modals (`WorkflowStepEditorModal.vue`), text fallback for color-only status badges, and a semantic heading hierarchy.

### 3.3 Live expression preview
- `ExpressionJsonEditor.vue` detects unresolved references but cannot evaluate expressions against sample data. Per memory `project_cc_expression_editor`, a TS-only evaluator/live-preview isn't possible today. Option: a ws "evaluate expression" endpoint the editor calls for preview — highest-value editor improvement.

### 3.4 Bulk actions, loading/empty states, error recovery
- No multi-select/bulk enable-disable-delete-rerun across workflows/runs. Add skeletons/spinners (only "Loading logs…" exists), richer empty states, and a "Retry" affordance on the error toast (`ToastHost.vue`) instead of console-only errors.

---

## Tier 4 — Test coverage & robustness

### 4.1 Backend test gaps
- Zero tests: `runinator-waker`, `runinator-supervisor`, `runinator-bootstrap`, and providers `aws`/`catalog`/`console`/`sql`. The waker (core timer/relay loop) being untested is the notable risk.
- Add DB round-trip/migration tests in `runinator-database` (none currently exercise sqlite↔postgres schema parity).

### 4.2 Frontend test gaps
- 0% component test coverage for the entire `components/workflow/` directory (canvas, node, step editor — the most complex, highest-LOC components). Utilities/stores are well covered (~80%); components are ~5%.

### 4.3 Panic hardening
- `expect()` clusters in `runinator-wdl/src/parser.rs:92-132` (parser state) and `runinator-ws/src/openapi.rs` (11 calls). Convert runtime-path panics to structured `RuntimeError`s per the error-dictionary convention.

---

## Recommended sequencing

1. **Start with 1.1 (tracing/metrics)** — it pays for itself immediately by making every other change observable, and it's mostly additive (no behavior change).
2. **Then 1.2 + 1.3** (DLQ/audit + backoff) — closes the silent-failure paths the tracing will expose.
3. **Pick one Tier-2 item** based on product need: `poll/while` (2.1) unblocks pack migration; authz (2.4) unblocks multi-user deployment.
4. **Dark mode + a11y (3.1/3.2)** can run in parallel as low-risk UX wins.

---

## Verification (per area, when implemented)

- **Backend:** `cargo fmt --all --check`, `cargo test -p <crate>`, then `cargo test --workspace` for shared-contract changes. Confirm the local stack still runs: `cargo run -p runinator-supervisor -- start|status|stop`.
- **Tracing/metrics:** start the supervisor stack, run a workflow, confirm spans correlate a run across ws/worker logs and `/metrics` reports counters.
- **DLQ/audit:** force a failing action, confirm a `dead_letters` row is written and surfaced via API/UI.
- **WDL changes:** round-trip a `.wdl` through compile→decompile→format and confirm idempotency.
- **Frontend:** existing Vitest path (`*.test.ts`) plus the Tauri build path; verify dark mode toggle and keyboard/focus behavior manually.

---

## Note

This roadmap is a survey for prioritization — no single item is fully specified for execution yet. Pick one (e.g. "do 1.1") to get a detailed, file-by-file implementation plan.
