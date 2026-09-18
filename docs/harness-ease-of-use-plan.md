# Runinator: harness bring-up ease-of-use program

## Context

Standing up the flint harness (dev / review / QA missions) worked, but cost far more than it should
have, and the cost was in Runinator rather than in flint. You asked for an analysis of why it wasn't
user-friendly, and approved all four workstreams that came out of it.

**The core finding: Runinator computes excellent diagnostics and then throws them away before they
reach the operator.** That's cheaper to fix than it felt, because the hard parts are already built.

Evidence from the bring-up:

| Measure | Value |
|---|---|
| Harness written | 36 files, 5,286 lines (3,878 REXRAP) |
| `flint-dev.rrx` | 2,208 lines — **63% is the `tests { }` block**, ~720 lines of duplicated fixtures (same profile object ×12) |
| Platform commits to make it work | ~20 (`6bf61d8e`..`b606af00`) |
| Commits chasing one bug class | 4 (`88fd9d30`, `85d2d857`, `c3e39169`, `7b419ab4`) |
| Raw `org_id == ctx.org_id` checks in ws handlers | 13 (9 in `execution_profiles.rs`) |
| Distinct causes returning `"execution profile bundle not found"` | 8, from one handler |

The recurring shape — a check that exists but is not connected to the operator's surface:

| Built and good | Where it's stranded |
|---|---|
| `worker_diagnostic = "no live worker matches the required labels: runner=desktop"` (`runinator-ws-authoring/src/handlers/adapters.rs:876-886`) | Dropped by the CLI renderer (`runinator-ctl/src/commands/orchestrations.rs:273-284`); survives only via `--json`. Also hardcoded empty in both DB read paths (`runinator-database/src/operations/orchestrations.rs:1104-1106`, `:1134-1136`) |
| `preview_event` — per-pipeline routes and actionable `validation_errors` (`runinator-engine/src/services/adapter_operations.rs:343-509`) | Secret-bound adapters only, and needs a hand-authored sample payload. For an execution-profile adapter, `adapters test` returns `{"job_id":…,"state":"queued"}` and exits 0, with **no command anywhere to fetch the result** |
| `deliveries` / `delivery` / `inspection` post-mortems | No CLI command exposes any of them |
| `render_snippet()` rustc-style carets (`runinator-rexrap-syntax/src/errors.rs:34-59`) | `RexRapError::Parse` carries no `Span`, so REXRAP001 squiggles the whole file (`runinator-lsp/src/diagnostics.rs:66-72`) |
| `IngressPolicy::validate_dispatches` cross-check precedent (`runinator-models/src/orchestration/ingress_policy.rs:120`) | Never applied to the scope join, which is the actual failure mode |
| `WORKFLOW_EFFECT_PROTOCOL_VERSION` (`runinator-engine/src/adapter_polling.rs:401`) | Not applied to the adapter-host contract |
| `unmarked_streams` seeding (`runinator-adapter-host/src/main.rs:1323`) | Lives in one GitHub function; **Jira still lacks it** (re-verified post-refactor) |

We already documented one of these ourselves — `docs/RUNINATOR_ENHANCEMENTS_CLAUDE_HARNESS.md:320-380`
says `adapters poll-status` "should say so in those terms." The server half shipped; the renderer was
never updated.

---

## Workstream A — stop discarding what you already compute

Hours of work; removes the single largest time sink.

- **A1** — print `worker_diagnostic` and `matching_worker_count` in `adapters poll-status`; fold the
  diagnostic into `LAST ERROR` when `last_error` is `None`. ~5 lines.
  → `runinator-ctl/src/commands/orchestrations.rs:273-284`
- **A2** — make `rexrap check|compile|format` run offline. Today only `workflows test` and
  `functions validate` are whitelisted, so everything else calls `fetch_auth_config()` and a new
  operator cannot lint a local file without a stack — failing with an error that names `auth/config`.
  → `runinator-ctl/src/main.rs:34-45`
- **A3** — `settings set` defaults to `--kind secret`; require it explicitly.
  → `runinator-ctl-core/src/cli.rs:559`
- **A4** — show `id` in `settings list`. Adapter apply is the one place in the product that demands a
  UUID for something every other verb addresses by `scope name`.
  → `runinator-ctl/src/commands/settings.rs:146-158`

## Workstream B — the real defects

Includes the three from the analysis plus two the rebase surfaced.

- **B1 — the `NEXT POLL == LAST ATTEMPT`, `last_error: None` stall.** Compound defect:
  `poll_one` returns `Ok(BatchSummary::default())` for profile-backed adapters
  (`adapter_polling.rs:93-98`), logged at `debug!` as "accepted 0 events"; `next_poll_at` only
  advances in `complete_orchestration_adapter_poll` (`:260-271`) which is unreachable without a
  worker, so the adapter re-claims in a hot loop forever; and `claimed_until` is set before
  `deadline_at`, so a re-claim rewrites `claimed_by` and the owner-matched expiry `UPDATE`
  (`runinator-database/src/operations/adapter_control.rs:369-376`) silently no-ops.
  Fix: order `claimed_until` after `deadline_at` (or drop the owner match), and back `next_poll_at`
  off on dispatch so a workerless adapter idles instead of spinning.
- **B2 — generic stream seeding.** `unmarked_streams` is a free function used only by the GitHub
  poller. `poll_jira_inner` reads `{instance}:issue` / `{instance}:comment` via `stream_checkpoint`
  and never seeds, so adding a Jira stream replays history — the exact failure the GitHub comment at
  `main.rs:1323` describes. Move stream declaration and seeding into `runinator-adapter-contract`
  (next to the `checkpoint` field it operates on) so it is structural rather than remembered.
  **Note:** `runinator-provider-support/src/polling.rs` is *pagination* (`poll_pages_async`,
  `PollPage`), not checkpoints — it is not the right home, and the adapter/provider direction
  boundary in AGENTS.md argues against putting inbound logic there.
- **B3 — adapter-host version skew.** Resolution falls through `RUNINATOR_ADAPTER_RUNNER_PATH`
  (unfiltered, unlike the two fallbacks) to `current_exe().parent()` to bare `$PATH`
  (`runinator-worker/src/effect_worker.rs:63-82`), with no version check anywhere. A day-stale binary
  polled "successfully" and produced 3 streams where current code produces 5. Fix: `is_file()`-filter
  the env branch, log the resolved path at spawn, and add `host_version` to `AdapterPollResponse`
  checked against the pinned `kind_version`.
- **B4 — `github_collect` page-walk trap (found during the rebase).** `next_cursor` is
  `(!exhausted && page_len == 100).then_some(page + 1)`. Any `GitHubOperation` whose request ignores
  `page` — `Reviews` is one — yields 10 identical pages (`GITHUB_MAX_PAGES = 10`), ~1,000 duplicate
  items, then **fails the whole poll** with `PageBudget { max_pages: 10 }` and retains the
  checkpoint, so the adapter stalls permanently. I avoided it for reviews by calling
  `client.execute` directly, but the trap is live for the next operation added. Fix: have
  `GitHubOperation` declare whether it paginates, and have `github_collect` refuse a
  non-paginating operation rather than walking it.
- **B5 — `poll_pages_async`'s `RepeatedCursor` guard is dead for page-number cursors.** Page numbers
  always increment, so `seen_cursors.contains(&next_cursor)` never fires and the only backstop is the
  page budget, which surfaces as a hard error. Either detect a repeated *page* (identical items) or
  document the guard as cursor-only so callers don't assume protection they don't have.
  → `runinator-provider-support/src/polling.rs`

## Workstream C — validate the joins at apply time

Kills the "accepted, stored, and inert" class. Three things I authored were validated, persisted,
hashed into the revision digest, and did nothing: a `routing_scope` on a GitHub adapter (Jira-only,
`main.rs:1924-1930`), an ingress scope no adapter can emit, and required worker labels no worker
satisfies.

- **C1** — validate pipelines offline at all. Neither `workflows test` nor `rexrap check` parses
  `blocks.pipelines`; it is dropped in `runinator-pack/src/source.rs:297-306` and `:573`, and
  `parse_pipeline_str` has exactly one caller (`workflows apply`, `source.rs:163`). A well-braced but
  invalid pipeline passes both offline checks and fails at apply — which is exactly what happened
  with the `#` comments. A green check that a later step rejects is worse than no check.
- **C2** — validate the scope join. Add `scope_template: Option<String>` to `AdapterKindMetadata`
  (GitHub `github:repository:{repository_id}`, Jira `{routing_scope}`), then
  `IngressPolicy::validate_reachability(&self, kinds)` beside `validate_dispatches`, called from
  `workflows apply` and `pipelines enable`. Also reject submitted adapter config keys a kind does not
  declare — `validate_definition` walks the declared schema and never the submitted keys
  (`runinator-ws-authoring/src/handlers/adapters.rs:312`).
- **C3** — static settings check. No pack in the repo declares a single `secret` slot, yet
  `sdlc-missions` consumes three tokens across 18 call sites, so the failure lands at the worker, in
  phase 1, inside a billable mission. Derive required slots from `import settings` plus
  `.secret.<name>` / `.config.<name>` references and report unset ones at apply.

## Workstream D/E — typed scope, real errors, doctor

- **D1** — make platform-vs-org visibility one typed decision. `resource_access.rs::owner_can_access`
  already encodes the rule correctly and **no handler calls it**. Add named `visible_for_read` /
  `visible_for_write` helpers and replace the 13 raw comparisons. The win is that the next handler
  must choose a policy by name instead of writing an `if`.
- **D2** — split `content`'s eight causes into distinct `ErrorDescriptor`s from
  `runinator-ws/src/errors.rs`, and move `audit()` so failures are recorded (it currently runs only
  on success, line 709). Response body to an untrusted caller stays vague; the log line names the rule.
  → `runinator-ws-authoring/src/handlers/execution_profiles.rs:629`
- **D3** — give `RexRapError::Parse` a `Span` so `render_snippet()` and the LSP work on REXRAP001.
- **E1 — `runinatorctl doctor`.** One command answering "is my harness wired?": scope reachability,
  worker-label satisfaction, secret resolution, profile readability *by the principal that will
  actually fetch it*, version skew, unset settings slots. Mostly assembly over data that exists.
- **E2** — expose `deliveries` / `delivery` / `inspection` as CLI commands.

## Workstream F — authoring volume (lower priority)

- **F1** — fixtures in `tests { }`: a named fixture plus per-case override. Cuts `flint-dev.rrx` by
  roughly a third.
- **F2** — a terminal scaffold. Today the documented derivation mechanism is `cp -R`
  (`docs/help/workflow-authoring.md:52`) and the only real scaffolding is the Tauri GUI's mission presets.
- **F3** — `rexrap format --check` in CI; fix the settings-import docs drift
  (`workflow-authoring.md:485` describes JSON; the CLI hard-rejects non-`.rrx`).

---

## Sequencing

A (hours) → B (correctness) → C (prevention) → D/E (structural + doctor) → F.

A and B are independent and can land together. C2 and E1 share the `scope_template` work, so C2
should land before E1. D1 should land before any new handler work to avoid a fourteenth raw check.

## Verification

- **A1** — `poll-status` against a label-mismatched adapter prints the diagnostic without `--json`.
- **A2** — `rexrap check` on a local file succeeds with no server running.
- **B1** — a workerless adapter shows a written `last_error` and an advancing `next_poll_at`.
- **B2** — adding a third Jira stream to an initialized adapter emits no backfill.
- **B4** — a unit test that passing a non-paginating operation to `github_collect` is refused rather
  than walked 10×.
- **C1** — the `#`-comment pack that passed `workflows test` and failed `apply` now fails `test`.
- **C2** — `mission.flint.review` against the GitHub kind fails reachability;
  `github:repository:1242743236` passes.
- **D1** — the cross-org isolation test in `runinator-ws/src/tests/execution_profiles.rs` stays green;
  add a platform-visible-to-org row per converted handler.
- **D2** — each of the eight causes yields a distinct descriptor, and failures are audited.
- **E1** — run against the live cluster (should be clean now) **and** against the three historical
  misconfigurations: the `runner=desktop` gap, the unreachable `mission.flint.review` scope, and the
  inert `routing_scope`. Naming all three is the acceptance test.

Standing checks per AGENTS.md: `cargo fmt --all --check`, `cargo clippy -p <crate> --all-targets`,
`cargo test -p <crate>`, then `cargo test --workspace` at the end of each cycle.

## Repo state

`main` is pushed and in sync at `b606af00`. The rebase onto your GitHub/Jira client consolidation is
resolved: the `issue_comment` poll now uses a new `RepositoryIssueComments` operation (the typed
client only modelled per-issue comments, which would be one call per PR), and the review block uses a
direct `client.execute` since `Reviews` is unpaginated. `cargo test --workspace` was still running at
push time — B4/B5 above came out of that work and are unrelated to its outcome.
