# SDLC workflow pack

This pack is a **status-driven pipeline**: the old monolithic *Ticket Work* flow is split into
four phase workflows, each an independent scanner keyed to a Jira status "inbox", tied together as
a first-class **Core SDLC** pipeline.

| Workflow | Trigger | Inbox status | Advances to |
|---|---|---|---|
| **SDLC: Development** | `cron "0 * * * *"` (pipeline head) | Ready for Development (budgeted) | In Review |
| **SDLC: Review** | chained on Development + cron | In Review | Ready for Testing |
| **SDLC: Deploy** | chained on Review + cron | Ready for Testing | In Testing |
| **SDLC: QA** | chained on Deploy + cron (terminal) | In Testing | Done / cleanup |

Each phase processes the tickets currently in its inbox and moves each to the next status; the
`chained` links fire the next phase as soon as a pass finishes (each phase also has a cron backstop,
so the chain is a latency optimization, not the only trigger). A ticket flows through the pipeline
over several scanner passes — the long parks of the old monolith (24h review gate, QA park) are now
**check-and-advance per pass**: a ticket that isn't ready yet is simply left in its status.

## Files

- `sdlc.wdlm` — the pack manifest (JSON): lists the four `wdl/*.wdl` workflows, the `pipeline/*.wdlp`
  pipeline file, and the `settings.wdls` bundle. This is what `runinatorctl workflows apply` loads.
- `wdl/sdlc-development.wdl`, `wdl/sdlc-review.wdl`, `wdl/sdlc-deploy.wdl`, `wdl/sdlc-qa.wdl` — the
  four phase scanners.
- `pipeline/core-sdlc.wdlp` — the **Core SDLC** pipeline: declares the four member workflows and the
  `Development -> Review -> Deploy -> QA` links. On import the web service upserts the pipeline and
  materializes each link as a managed `chained` trigger stamped with the pipeline id.
- `settings.wdls` — seeds every `config.*` value and `secret.*` token the workflows reference. Every
  slot ships as a `<<insert here>>` placeholder; replace each with a real value for your org.

## How the phases share state

Because a `chained` trigger carries **no per-ticket data** (the runtime chaining engine is
fire-and-forget), the phases coordinate entirely through Jira status and a few deterministic
conventions:

- **Jira status is the handoff.** Each phase's inbox is a JQL (`jira.*_jql`). Development moves a
  ticket to *In Review*, Review merges and moves it to *Ready for Testing*, Deploy dispatches and
  moves it to *In Testing* (the deploy marker — an In Testing ticket has already been deployed), and
  QA reacts to the human QA outcome.
- **Deterministic worktree.** The git worktree lives at `git.worktree_root/<TICKET-KEY>`.
  Development creates it; Review/Deploy re-attach the same path (`git.worktree` is create-or-attach);
  QA runs `git.cleanup` on finished tickets. Every worktree-touching node is pinned with
  `.runner("sdlc")` so it lands on the one worker that holds the checkouts — run a single worker with
  `RUNINATOR_WORKER_LABELS=runner=sdlc`. If a phase ever lands without the worktree, `git.worktree`
  re-materializes it from the remote branch.
- **PR by branch.** Phases re-obtain the PR with `github.create_pr` (create-or-update finds the open
  PR for the branch instead of opening a duplicate), so no PR number needs to cross the chain.
- **Overlap safety.** Each phase holds a per-phase `mutex` so an overlapping cron/chained fire never
  double-picks a ticket.
- **Cooldown.** Each phase opens with a per-phase `cooldown "sdlc-<phase>" every 300s` (the first
  node, before the mutex). A pass that starts within 5 minutes of the prior pass short-circuits to a
  clean success without scanning, so a near-simultaneous cron + chained fire collapses to one pass
  instead of hammering Jira/GitHub back-to-back. cron (every 30 min / hourly) remains the baseline
  cadence, and a chained fire that lands after the window still runs — a scanner completes in seconds
  when its inbox is empty, so Development completions (which take minutes) always clear the window
  before chaining the next phase.

## Config and secrets

Workflows read `config.*` for non-sensitive shared values (resolved eagerly in the web service,
freely interpolatable) and `secret.*` for the three tokens (`secret.jira.token`,
`secret.github.token`, `secret.slack.token`, lowered to a `secret://scope/name` reference resolved
late at the worker). Import the whole pack — workflows, settings, and pipeline — in one step:

```bash
runinatorctl workflows apply ./packs/sdlc/sdlc.wdlm   # workflows + settings + Core SDLC pipeline
runinatorctl settings list                            # config + secret slots, no values
runinatorctl settings set jira token <api-token> --kind secret   # fill real token values
```

`workflows apply` also accepts a directory of `.wdl` files (with any sibling `settings.wdls` and
`*.wdlp` pipeline files) as a pack.

## Retry policy

Network-bound nodes carry `.retry(...)` with jittered exponential backoff and an error class chosen
by side-effect safety: **reads** (`jira.poll/search/comments`, `github.reviews/checks_summary/
workflow_runs`, `git.diff`) retry `on: any`; **idempotent writes** (`git.push/worktree/cleanup`,
`slack.send_message`) retry `on: failure` only; **non-idempotent writes** (`github.create_pr/
merge_pr/dispatch`, `jira.transition`, `git.commit`, comment/reviewer/assignee calls) carry no retry.
The Claude agent steps (`ai-command.claude_code`) are deliberately not retried — expensive and
non-idempotent.
