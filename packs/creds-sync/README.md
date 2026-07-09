# Worker Credential Sync Pack

Two hourly workflows that copy the operator's **local** Claude Code and AWS logins into the
Kubernetes Secrets the worker pods mount, so cloud workers act as the logged-in identity.

- `wdl/update-claude-auth.wdl` — syncs the Claude Code login into the `claude-credentials` Secret.
- `wdl/update-aws-auth.wdl` — refreshes the AWS SSO cache and syncs it into the `aws-sso-cache` Secret.

Both delegate to the existing `scripts/sync-secrets.sh` engine (see
`tools/runinator-secret-sync`), scoped per credential via `secret-sync.claude.json` /
`secret-sync.aws.json`. The sync spec is the single place that grows to carry future **API-key**
credential files — add a new job, no workflow change.

## How it runs on the right machine

Each node uses `.runner("creds-sync")`. The reducer routes a node with a required label to a live
worker advertising that label, and **parks then fails** (on the node `timeout`) when none is
connected. So these workflows only ever execute on a worker you start on the operator's workstation,
either a standalone worker:

```bash
RUNINATOR_WORKER_LABELS=runner=creds-sync \
RUNINATOR_CONSOLE_ALLOW_INTERACTIVE=1 \
  cargo run -p runinator-worker -- --advertise-host <host>
```

`RUNINATOR_CONSOLE_ALLOW_INTERACTIVE=1` opts this terminal-attached worker into `interactive` console
commands; without it the console provider rejects them (the cloud-worker default — see the interactive
note below). The `runinator-desktop-agent` tray app sets it for you.

or `runinator-desktop-agent` (the tray app) with `runner=creds-sync` added to its "Extra labels"
field — it registers the full built-in provider catalog (including `console`, `aws`) unconditionally
and marks itself interactive-capable, so either workflow can route there once the label matches; it
just stays `exclusive`, so it never picks up unrelated general-pool work in the meantime.

Either surface's "which broker transport" is a separate choice from "what kind of worker this is" —
by default both relay through `runinator-ws` (`--broker-backend ws` for the standalone binary, or
`runinator-desktop-agent`'s "Broker connection: Via web service" mode), but either can instead connect
straight to the broker (`--broker-backend tcp`/`rabbitmq`, or the agent's "Direct" mode) if that
worker is actually on the broker's trusted network.

That worker must have:

- the local logins the sync engine reads — the Claude Code Keychain item (`keychain-export`) and
  `~/.aws` (SSO profile + `~/.aws/sso/cache`), and
- a working **kubeconfig** (EKS exec-auth) pointing at the target cluster, since the sync writes
  namespaced Secrets.

Both nodes run `console.run(..., interactive: true)`, so the command is attached to the operator's
**desktop session** rather than run headless: a lapsed AWS SSO session can complete `aws sso login`
in a browser, and `keychain-export` can satisfy the macOS Keychain access dialog. Run the
`creds-sync` worker in that interactive session (a foreground terminal, or the
`runinator-desktop-agent` tray app) — a fully headless/daemon worker cannot answer those prompts and
the run fails on the node `timeout`. In `interactive` mode the command's stdout/stderr are not
captured or streamed to the UI.

`interactive: true` is gated to a worker that runs in a desktop session: the `runinator-desktop-agent`
sets `RUNINATOR_CONSOLE_ALLOW_INTERACTIVE=1`, so the console provider permits it there. A headless
cloud worker never sets that flag, so an interactive console command routed to one is rejected up
front with `CONSOLE008 - Interactive console is only available on a desktop worker agent` instead of
hanging with no terminal. Together with `.runner("creds-sync")`, this keeps these jobs on the
operator's machine.

If that worker is offline when a scheduled run fires, the run fails — by design.

## Working directory

Both nodes run `scripts/sync-secrets.sh` with a **relative** path, so the `creds-sync` worker must run
console commands from a checkout of this repository. Point it there:

- **`runinator-desktop-agent`** — set the "Working directory" field to the repo checkout (e.g.
  `/Users/you/Documents/GitHub/runinator`). The agent exports it as `RUNINATOR_CONSOLE_WORKING_DIR`.
- **standalone `runinator-worker`** — set `RUNINATOR_CONSOLE_WORKING_DIR=/path/to/runinator` (or just
  launch the worker from inside the checkout).

The pack itself needs no settings.

## Import

```bash
runinatorctl workflows apply packs/creds-sync
```
