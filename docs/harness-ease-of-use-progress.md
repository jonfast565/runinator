# Harness bring-up ease-of-use program — progress

Tracks the workstreams from the "Runinator: harness bring-up ease-of-use program" plan (workstreams
A–F). The plan itself is the source of truth for *why* each item exists; this file records only what
has landed and what has not.

**State at this commit:** A through F are implemented. Final verification for E and F is recorded in
the completing commit rather than this historical handoff note.

## Done

### Workstream A — stop discarding what you already compute

- **A1** `adapters poll-status` prints `worker_diagnostic` and `matching_worker_count` without
  `--json`. A new `WORKERS` column shows `<n> live for <selector>`, and the diagnostic takes the
  `LAST ERROR` cell when `last_error` is `None`.
  → `runinator-ctl/src/commands/orchestrations.rs`
- **A2** `rexrap check|compile|format` run offline. `Commands::RexRap` joins the pre-auth arm in
  `run_process`, so a local file lints with no web service running.
  → `runinator-ctl/src/main.rs`
- **A3** `settings set --kind` is required rather than defaulting to `secret`. `get`/`delete` keep
  their default: a wrong kind there yields "not found", not silent corruption.
  → `runinator-ctl-core/src/cli.rs`
- **A4** `settings list` shows `id`, which adapter apply demands.
  → `runinator-ctl/src/commands/settings.rs`

Verified by hand: `rexrap check` succeeds with no server; `settings set` rejects a missing `--kind`.
The two MCP schema tests that asserted the old default were updated.

### Workstream B — the real defects

- **B1** The `NEXT POLL == LAST ATTEMPT`, `last_error: None` stall. Three parts:
  - A poll attempt's `deadline_at` now comes from the claim it was taken under
    (`PollAttemptRequest::deadline_at`), so the attempt can no longer outlive its claim.
  - The expiry sweep matches by `adapter_id` instead of `claim_owner`; an expired `claimed_until`
    already means no live poll owns the row, and the owner recorded there is whoever claimed last.
  - New `defer_orchestration_adapter_poll` pushes `next_poll_at` out when a poll is dispatched to a
    worker, so a workerless adapter idles at its interval instead of re-claiming every lease.
  → `runinator-engine/src/adapter_polling.rs`, `runinator-database/src/operations/adapter_control.rs`,
    `runinator-database/src/operations/orchestrations.rs`, `runinator-store/src/roles/orchestrations/`
- **B2** Generic stream seeding. `StreamCheckpoints` / `StreamScope` now live in
  `runinator-adapter-contract`, next to the `checkpoint` field they operate on. Both the GitHub and
  the Jira poller declare their streams through it, so a stream added to a running adapter seeds
  instead of replaying history — the Jira gap is closed structurally rather than by remembering.
  The host-local `stream_checkpoint`/`stream_checkpoints`/`existing_streams`/`advance`/
  `unmarked_streams` helpers are gone, and their tests moved to the contract crate.
- **B3** Adapter-host version skew. `RUNINATOR_ADAPTER_RUNNER_PATH` is now `is_file()`-filtered like
  the two fallbacks; the resolved path is logged at spawn; `AdapterPollResponse` carries
  `host_version` and `kind_version`, stamped once on the way out of the host, and the engine fails a
  batch whose `kind_version` disagrees with the revision's pin.
- **B4** `github_collect` page-walk trap. `GitHubOperation::paginates()` declares whether a request
  carries `page`, and `github_collect` refuses a non-paginating operation instead of walking it ten
  times. Covered by two tests in the adapter host.
- **B5** `poll_pages_async`'s `RepeatedCursor` guard documented as cursor-only, with a test asserting
  that a page-number walk is bounded only by its budget. B4 is what actually prevents the trap.

### Workstream C — validate the joins at apply time

- **C1** Pipelines are validated offline. `runinator_pack::source::compile_pipeline_block` applies
  exactly the requirements apply enforces, and both `rexrap check` and `workflows test` call it.
  Verified: the `#`-comment pipeline that passed `rexrap check` and failed at apply now exits 1 with
  a REXRAP001 caret.
- **C2** Scope-join validation. `AdapterKindMetadata::scope_template` declares the shape of the scope
  a kind emits (GitHub `github:repository:{repository_id}`, Jira `{routing_scope}`, Slack
  `{channel}`); `IngressPolicy::validate_reachability` checks a policy's scope against the installed
  kinds and is called from workflow upsert and from pipeline create/update/enable. A kind that
  declares no template never blocks an apply, and an unreachable adapter host is not a failure.
  Also: `validate_definition` now rejects a submitted configuration key the kind does not declare —
  the inert `routing_scope` on a GitHub adapter.
- **C3** Static settings check. A pack apply reports the setting slots its workflows reference that
  the organization has not provisioned (`PackImportResult::unresolved_settings`), and
  `workflows apply` prints them as a warning with the command to set each. The apply still succeeds:
  a portable pack may legitimately reference settings provisioned afterwards.

### Workstream D — typed scope, real errors

- **D1** `AuthContextExt::visible_for_read` / `visible_for_write` replace all 13 raw
  `org_id == ctx.org_id` comparisons (9 in `execution_profiles.rs`, 3 in `credentials.rs`, one each
  in `files.rs` and `notifications.rs`). Read admits a platform-owned record; write requires the
  caller's own tenant. Both admit a platform administrator, which is the one documented
  administrative short-circuit.
- **D2** The nine causes in `execution_profiles::content` each carry a distinct `ErrorDescriptor`
  (`RUNI191`–`RUNI199`, in the new `runinator-ws-core/src/errors.rs`, re-exported into the
  `runinator-ws` dictionary). The response body an untrusted caller sees is unchanged; the log line
  and the audit entry name the rule. `refuse_bundle` also records a `Denied` audit entry, so a
  refusal is now recorded at all — `audit()` previously ran only on success.
- **D3** `RexRapError::Parse` carries an optional `Span`, taken from pest's own reported location.
  `render()` produces a caret snippet for REXRAP001 and the LSP underlines the offending token
  rather than the whole document. `RexRapError::span()` exposes the position to other callers.

## Completed after the original handoff

### Workstream E — doctor and post-mortems

- **E2** Read-only authorized HTTP and CLI surfaces expose deliveries, individual delivery
  post-mortems, polling attempts, and the inspection gate.
- **E1** `runinatorctl doctor` assembles installed kind metadata, current adapter revisions, poll
  diagnostics, profile health/readability, setting bindings, and stored workflow/pipeline ingress
  policies into one actionable harness report.

### Workstream F — authoring volume

- **F1** `tests { }` suites accept named fixtures and recursively merge per-case overrides.
- **F2** `runinatorctl workflows scaffold` creates a complete starter pack offline and refuses to
  overwrite a non-empty directory.
- **F3** CI checks every checked-in pack with `rexrap format --check`, and the settings-import guide
  now documents the `.rrx`-only contract.

## Verification for the completing commit

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets`
- `cargo test --workspace` — **not yet run against any of workstreams B through D.** Expect fallout
  in the ws test suites from D1 in particular; the plan calls for the cross-org isolation test in
  `runinator-ws/src/tests/execution_profiles.rs` to stay green, plus a platform-visible-to-org row
  per converted handler.
- `cargo check --workspace --all-targets` — plain `--workspace` passed, but test targets were not
  compiled after `AdapterKindMetadata` and `PackImportResult` gained fields.
- The completing change bumps the workspace minor version.
