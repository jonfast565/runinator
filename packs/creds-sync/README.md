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
  cargo run -p runinator-worker -- --advertise-host <host>
```

or `runinator-desktop-agent` (the tray app) with `runner=creds-sync` added to its "Extra labels"
field — it registers the full built-in provider catalog (including `console`, `aws`) unconditionally,
so either workflow can route there once the label matches; it just stays `exclusive`, so it never
picks up unrelated general-pool work in the meantime.

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

If that worker is offline when a scheduled run fires, the run fails — by design.

## Settings

Set `config.creds_sync.workspace` in `settings.wdls` to the absolute path of this repository checkout
**on the `creds-sync` worker** (the workflows invoke `scripts/sync-secrets.sh` from there).

## Import

```bash
runinatorctl workflows apply packs/creds-sync
```
