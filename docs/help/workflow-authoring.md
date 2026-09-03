# Workflow authoring and import

Use this guide to compile, import, test, simulate, and edit REXRAP workflow packs. It also covers runtime control-flow, pipelines, correlated orchestration, ingress adapters, notifications, schedules, and settings references. For the execution model behind these features, see the [REXRAP runtime model](../../runinator-rexrap/docs/runtime.md).

## Workflow Import

`runinatorctl workflows apply <path>` imports a workflow pack in one shot. The
path can be a `.rrx` source file or directory (each source is a unified REXRAP
container with workflow, pipeline, settings, package-manifest, and test blocks),
or one standalone workflow-definition JSON file. Compiled workflow bundles are
not accepted. The checked-in local supervisor config applies `packs/hello-world`.
File/session authentication is configured as an Execution Profile in Command Center and bound to
provider calls with `@profile("name")`; collection requires approval in the desktop agent. To load
ordinary scalar credentials and config, import an `.rrx` source containing a `settings` block with
`runinatorctl settings import <file>`. Each entry carries a `kind`
(`secret` — the default — or `config`) and a `value`; secret values stay
encrypted and resolve late at the worker, while config values are arbitrary JSON
read by the web service. You can seed the app-data workflow pack from the
repository sample if needed:

```bash
mkdir -p ~/.runinator/workflows
cp -R packs/hello-world ~/.runinator/workflows/hello-world
```

Compiled JSON workflow packs are never authored or checked in. Use a unified
`.rrx` source or a directory containing one or more `.rrx` sources for pack imports.

Because `workflows apply` overwrites stored definitions wholesale, every accepted
definition is also captured as an immutable revision. `runinatorctl workflows
revisions <workflow>` lists that history (revision number, version, source —
`ui`/`pack`/`api`/`duplicate`/`rollback` — author, and timestamp), `workflows
revision <workflow> <n>` prints one revision with the definition it captured, and
`workflows rollback <workflow> <n>` restores it. A rollback re-validates the old
definition against the current provider catalog and saves it as a *new* revision,
so nothing is overwritten and the rollback itself stays in the history. The same
history is available per workflow in the command center's Workflow Settings
dialog, with a diff between any two revisions. An unchanged re-apply records
nothing, so a pack imported on a schedule does not bury real edits.

`runinatorctl workflows dev <path>` runs the same client-side pack compile and
compiled zip upload in a watch loop. It watches the selected `.rrx` source or
directory, adjacent sources, and an optional `--json-file`. When `--run` is
provided, it starts that workflow after each successful import and refreshes the
run detail until the run reaches a terminal state.

`runinatorctl workflows test <path>` dry-runs a pack against `tests { ... }`
blocks in `.rrx` sources
entirely client-side — no server or broker. It compiles the pack, then simulates each
workflow's deterministic condition/switch/toggle/percentage routing, stubbing task nodes
with mocked outputs, and asserts on the branch
taken and final outputs. Test cases live in an RRX `tests` block and provide a name,
input, config, mocked task outputs, and assertions on status, reached nodes, branches,
and final output. Every `.rrx` source in the pack is considered automatically, or pass
additional sources with `--tests`. The command
exits non-zero when any case fails, so it drops straight into CI.

The same simulator also backs a server-side dry-run: `POST /workflows/simulate`
(`WorkflowSimulateRequest`: `{ workflow, inputs?, replay_run? }`) walks a workflow
against live config — publishing no actions — and returns the routed path, per-node
status, branch targets, and final output. Pass `replay_run` to replay a prior run's
recorded node outputs so the walk follows the branches that run actually took. A
saved workflow requires `Run` permission; an unsaved draft only an authenticated
caller; `replay_run` is additionally gated on that run's workflow. The command
center surfaces it as a **Dry run** button in the workflow editor toolbar.

For the checked-in local stack only, `bash scripts/run-local.sh sync`, `dev`,
and `smoke-sync` will default `RUNINATOR_API_KEY` to the same seeded dev
service key when talking to `http://127.0.0.1:8080/` and no explicit
`RUNINATOR_API_KEY` is already set. Pointing those helpers at another stack
still requires that stack's own credentials.

For a minimal smoke import, use `./packs/hello-world/hello-world.rrx`. It
contains one REXRAP workflow that runs a single built-in console action and is wired
into `bash scripts/run-local.sh smoke-sync` for an import-and-run check against
an already running local stack.

### Editor integration (language server)

`runinator-lsp` is an editor-agnostic Language Server for `.rrx` files: live diagnostics,
provider/action completion (from live service metadata), hover, formatting, and an optional
apply-on-save that imports the pack into a running web service — the editor-native counterpart of
`runinatorctl workflows dev`. Build it with `cargo build -p runinator-lsp --release` and point your
editor at the binary. See [`runinator-lsp/README.md`](../../runinator-lsp/README.md) for the VS Code
extension and Neovim/Zed setup. The pack compile-to-bundle logic shared by the CLI and the server
lives in the `runinator-pack` crate.

Workflow syntax now includes richer declarative control-flow nodes:

- `switch` routes by ordered cases and an optional default target.
- `parallel` starts branch roots, with branch nodes returning to a `join`.
- `join` waits for named upstream nodes using `all`, `any`, or `first_success`.
- `try` runs a body, optional catch, and optional finally node; those nodes transition back to the `try` controller.
- `map` runs one target node for each resolved item and exposes the current item under `workflow.state.map`.
- `race` starts branch roots until one satisfies the winner policy, then cancels the still-running losing continuations and effects.
- `emit` records structured node output without calling a provider.
- `reentry` allows explicit bounded cycles back to a node and can route to `on_exhausted`.

#### Retry and compensation

An action node carries two failure-handling policies, both editable in the step editor and both
previewed there so the effect is visible before saving:

- **Retry** (`@retry(attempts, backoff: 2s, max: 60s, jitter: true, on: failure)`) re-arms the
  action's durable effect while its continuation remains parked. The delay before attempt *n+1* is
  `clamp(backoff * 2^(n-1), backoff, max)`, optionally
  jittered into the lower half to spread a retry storm. `on` narrows which terminals are eligible
  (`any`, `failure`, `timeout`) so a long expensive action is not blindly re-run on a timeout. The
  editor renders the resulting schedule, since exponential backoff under a cap is hard to eyeball.
  The VM receives only the eventual settlement, so retries do not re-enter graph control flow.
- **Compensation** (`compensate provider.fn(args)`) is a saga rollback. Once the node has succeeded,
  a run that later reaches `fail` calls the compensating action; compensations unwind in reverse
  order and are best-effort, so one that fails does not stop the unwind. The clause is the same call
  form as the forward action and carries its own attributes.

```rexrap
@retry(3, backoff: 5s, on: failure)
let deploy = k8s.apply(manifest: params.manifest)
    compensate @timeout(300s) k8s.rollback(release: deploy.release)
```

#### Triggers and workflow chaining

Workflows declare triggers in the REXRAP header, materialized from `metadata.triggers`
on import (pack-managed, `metadata.managed_by = "rexrap"`):

- `trigger cron "<expr>"` schedules the workflow.
- `trigger on_success | on_failure | on_complete workflow "<name>"` chains another
  workflow: when this run reaches that terminal state, the named target starts a new
  run. `on_failure` matches `Failed`/`TimedOut` (not a manual `Cancel`).

```rexrap
trigger cron "0 * * * *"
trigger on_success workflow "Downstream Report"
```

Chaining is event-driven from the VM run's terminal settlement (not the best-effort
`events` channel), fired exactly once per (trigger, source-run) via a durable
dedupe table, and cycle-bounded by a `chain_depth` cap. Only top-level runs fan out
chains — subflow/map children do not. Chaining does **not** replace `subflow`: a
subflow is a synchronous child with a return path, while a chain starts an
independent downstream run.

The command center's top-level **Pipelines** tab visualizes chains as a DAG — one
node per workflow, one edge per chained trigger — and lets you author them by
dragging between workflows, editing an edge's `on` selector, or enabling/disabling
and deleting chains through the normal trigger CRUD.

#### Generic ingress lifecycle

Workflows and pipelines can declare a provider-neutral admission policy. Runinator only receives
an already-authenticated opaque event; Jira/GitHub signatures and payload interpretation remain in
the caller or provider adapter. The policy selects an action from the event type and the durable
admission lifecycle:

```rexrap
ingress scope "release.lifecycle" {
    on "created"  when unbound  -> start
    on "observed" when active   -> record
    on "updated"  when active   -> queue
    on "canceled" when active   -> interrupt
    on "observed" when terminal -> record
    on "reopened" when terminal -> requeue
}
```

`start` creates generation one. `record` appends audit provenance without changing the active run.
`queue` appends to the admission's durable FIFO; terminal settlement promotes exactly one head event
into the next generation and startup failures release that claim without reordering. `interrupt`
uses the workflow external-interrupt path, while a pipeline interrupt cancels the pipeline and every
active member. `requeue` compare-and-swaps a terminal admission into its next generation. A missing
route is an explicit no-op/rejection and never starts work. `(source, event_id)` is deduplicated in
the admission ledger, so retries receive the original disposition and run reference.

`dispatch "<intent>"` is the pipeline-only handoff to a correlated orchestration policy. It does
not start a second pipeline: it records a durable intent against the existing scope/correlation
binding, then the policy resolves it by priority and coalescing rules.

Submit events to `POST /workflows/{id}/ingress` or `POST /pipelines/{id}/ingress`. Authenticated
operators can inspect the owner at `GET /ingress/admission?scope=…&correlation_key=…` and its ordered
timeline at `GET /ingress/admission/events?scope=…&correlation_key=…`.

#### Correlated pipeline orchestration

Place an `orchestration { ... }` block inside a pipeline when later events must control one durable,
ticket- or resource-scoped execution rather than merely start another run. The engine creates one
binding per ingress scope, correlation key, and generation; it pins the pipeline revision and tracks
the active epoch and phase. `intent` declarations give normalized external control events a priority,
optional coalescing period, effect (`terminate`, `suspend`, `resume`, `supersede`, `observe`, or
`signal`), and restart/stop behavior. `budget` declarations bound retry classes and select a terminal
handoff. `phase` declarations project selected workflow output fields into the durable binding and can
lease a labeled workspace for reuse across member workflows.

```rexrap
pipeline "Ticket automation" {
    ingress scope "ticket.lifecycle" {
        on "ready" when unbound -> start
        on "cancel" when active -> dispatch "stop"
    }

    orchestration {
        intent "stop" effect terminate priority 100
        budget "transient" attempts 3 exhausted pause via "handoff"
        phase "implement" {
            evidence from "/evidence"
            workspace scope "source" reuse lease 30m labels { "capability": "git" }
        }
    }

    workflow "implement"
    workflow "handoff"
}
```

The **Orchestrations** command-center view and `runinatorctl orchestrations` expose bindings,
timelines, aliases, and operator intents; `runinatorctl orchestrations adapters` manages the inbound
adapter definitions and reports polling health. See
[`packs/autonomous-development`](../../packs/autonomous-development/README.md) for the complete
ticket-scoped example.

#### Webhook and polling adapters

Adapters are inbound translators: they authenticate or fetch provider events, normalize them, and
hand them to the durable ingress policy above. Providers are outbound executors used by workflow
steps. They share a plugin-style boundary, but they serve opposite directions and are not
interchangeable.

GitHub and Jira adapters accept `"transport": "webhook"` (the default) or
`"transport": "polling"`. A polling revision is scheduled durably by the engine, so embedded and
standalone engine deployments use the same claim/checkpoint path and multiple replicas do not poll
the same revision concurrently. GitHub polling requires `configuration.repositories` plus an
`access_token` Secret binding. Jira polling requires `instance_id`, `base_url`, `email`, and `jql`
plus an `api_token` Secret binding. Both accept `poll_interval_seconds` from 30 through 3600,
defaulting to 60.

The first successful poll establishes a high-water checkpoint without admitting historical events.
Later polls normalize events through the same pipeline-ingress service as webhooks.

Checkpoints are per stream — pull requests, workflow runs, check runs, Jira issues, and Jira
comments each carry their own high-water mark — because those streams advance independently and one
shared mark would let a busy stream drag the watermark past events a quiet one had not emitted yet.

A polling adapter enumerates everything upstream has, so most batches contain events no pipeline
admits. Those are skipped and counted, not treated as failures: failing the poll would leave the
checkpoint unadvanced and replay the identical batch forever, so one unroutable event would stall
the adapter permanently. Only a fault that would recur for every event — the adapter host being
unreachable, the store being down — abandons the batch and retries. Rate limits are honored
separately: the adapter returns the provider's `Retry-After` and the checkpoint is left untouched.

Inspect schedule and health with `runinatorctl orchestrations adapters poll-status <adapter-id>` or
`GET /orchestrations/adapters/{id}/poll-status`; `last_error` carries the most recent failure.
Transport and identity configuration become immutable after the adapter admits its first
correlation.

Both webhook verification and polling run in the `runinator-adapter-host` process, which must be
running for any adapter to work — see [Kubernetes](kubernetes.md) and
[Local development](local-development.md).

#### Failure alerting

Workflows declare alerting policies in the REXRAP header, materialized from
`metadata.notifications` on import the same pack-managed way triggers are:

- `notify on failure -> <channel> "<target>"` fires when a run ends `Failed`/`TimedOut`.
- `notify on retry_exhausted -> ...` fires for the node that used up its retry budget.
- `notify on sla -> ... after <duration>` fires when a run stays open past the threshold.
- `notify on parked -> ... after <duration>` fires when a run sits waiting/approval/input/blocked
  past the threshold.

`<channel>` is `slack`, `email`, or `app` (in-app only). `severity info|warning|critical`
defaults to `warning`; `with { ... }` overrides the generated provider configuration
(notably the credential reference); `disabled` imports the policy switched off. The two
duration events require `after`, and the compiler rejects them without it — a policy the
scanner could never match is a silent failure during an incident.

```rexrap
notify on failure -> slack "#oncall" severity critical
notify on sla -> email "ops@example.com" after 30m
```

Policies can equally be managed from the command center's **Notifications** tab
(capability `notifications:manage`); pack-managed rows are read-only there because an
import reconciles them. Emission happens in `runinator-engine` at the terminal
transition, plus a periodic scanner for the duration events. The engine never speaks a
vendor protocol: an in-app policy writes the notifications row, and every other channel
is enqueued as a provider-effect command on the notification delivery outbox so a worker delivers it through the
`runinator-provider-slack` / `-email` provider like any other action. Delivery attempts
are tracked per notification and readable at `GET /notifications/{id}/deliveries`.

#### Schedule policy: concurrency, catch-up, and freeze windows

A cron trigger fires without asking whether the last run finished. Two REXRAP header
clauses change that, and both are evaluated at the claim point — the loop declines to
create the run rather than creating one that immediately parks:

```rexrap
trigger cron "0 * * * *" catchup fire_all max 10
concurrency 1 on_conflict queue
```

`concurrency <n> on_conflict <policy>` caps overlapping runs of the workflow. The
policies:

- `skip` (the default when the clause is omitted) — drop the slot, record the firing so
  it is never retried, and move the schedule on.
- `queue` — leave the slot due and re-evaluate next tick, so it fires as soon as capacity
  frees up. Nothing is created while blocked: the schedule itself is the queue, so a
  backed-up workflow costs no runs and no wake-queue entries.
- `cancel_previous` — cancel the workflow's in-flight runs, then start this one.
- `allow` — the historical behavior, and what an absent header means.

`catchup <policy>` decides what happens to slots that came due while nothing was firing
them (engine downtime, a freeze window, or a `queue` policy holding the schedule back):

- `fire_once` (default) — collapse the whole backlog into one run and re-anchor.
- `fire_all [max <n>]` — replay each missed slot as its own run, at most `max` per pass
  (25 by default) so a week of downtime cannot flood the run table in one tick.
- `skip [grace <duration>]` — abandon slots more than `grace` late (60s by default) and
  re-anchor. The grace matters: every firing is slightly late, so without it `skip` would
  drop every run.

The compiler rejects `grace` on a non-`skip` policy and `max` on a non-`fire_all` one
rather than storing a knob the runtime never reads.

**Disabling a workflow** stops every path that starts it automatically, not just its own
schedule: its cron triggers drop out of the due claim, and a chained trigger pointing at
it declines to start it. Both leave the link and the due slot in place, so re-enabling
resumes on the same terms a freeze window does. A trigger keeps its own `enabled` flag on
top of this — turning off one schedule is not the same as turning off the workflow. Only
an explicit run (`runinatorctl triggers run`, the Run button, a backfill) still starts a
disabled workflow, because that is somebody asking for it by hand.

**Freeze windows** suspend firing over a date range, independently of any pack. A window
scopes to one workflow, one org, or the whole platform (and platform/org windows also
hold pipeline cron triggers). Frozen triggers are excluded from the claim in SQL, and
their due slot is deliberately left in place — when the window lifts, the trigger's
catch-up policy decides whether the missed slots replay. Manage them under the command
center's **Schedules** tab or from the CLI, both gated on the `schedules:manage`
capability:

```bash
runinatorctl freeze create "December freeze" --from 2026-12-20T00:00:00Z --to 2027-01-02T00:00:00Z
runinatorctl freeze list --active
```

**Backfill** replays a cron trigger's slots across a past range. Slots the loop already
fired keep their original run — the firing row is the same uniqueness gate the loop
claims through — so an overlapping range is safe to re-issue:

```bash
runinatorctl triggers backfill <trigger-id> --from 2026-08-01T00:00:00Z --dry-run
```

REXRAP references resolve runtime values into action arguments. Alongside `params.*`,
`prev.*`, `run.*`, and bare node-output names, two roots read from the unified
settings store:

- `config.<scope>.<name>` — non-sensitive JSON, resolved eagerly by the web
  service. It interpolates freely (e.g. `"${config.api.base}/v2"`) and can drill
  into stored JSON (`config.api.settings.url`).
- `secret.<scope>.<name>` — sensitive values, lowered to the `secret://scope/name`
  form and resolved late at the worker so plaintext never reaches the web
  service, database, or broker. A secret must be passed as a whole argument value
  (it cannot be interpolated mid-string).

Stored settings are typed. Config values are validated on write against a declared
JSON-schema (required once per `scope/name`, then reused for value-only updates);
a value that does not match the schema is rejected. Secrets are validated as
non-empty strings. Manage them with `runinatorctl`:

```bash
# declare + store a config value (schema required on first write)
runinatorctl settings set api base '"https://api.example.com"' \
  --kind config --schema '{ "type": "string" }'
# later value-only update reuses the stored schema
runinatorctl settings set api base '"https://api.example.com/v2"' --kind config

# store a secret (string)
runinatorctl settings set github token "ghp_xxx"

# read a value from a file instead of passing it inline
runinatorctl settings set github deploy-key --value-file ./id_ed25519

# bulk import secrets and config from a bundle file
runinatorctl settings import ./secrets.json

runinatorctl settings list            # all settings, no values
runinatorctl settings get api base --kind config
```

The import file is a `{ "secrets": [...] }` document; each entry carries
`scope`, `name`, and `value`, plus optional `kind` (`secret` or `config`) and
`schema`. Existing entries are only overwritten when an incoming `updated_at` is
strictly newer.

### Interactive terminal input

REXRAP can request one line of input directly with the typed `console.input` function. It runs as
an action, so the prompt and entered line appear in the selected action's **Step Output** terminal,
the normal action timeout continues while it waits, and the result can be used by later statements:

```rexrap
@runner("desktop")
@timeout(300)
let answer = console.input(prompt: "Which environment should I deploy?")

output (answer.value)
```

`console.input` must run on a desktop worker and returns `{ value: string }`. The completed value is
part of the durable action result so the workflow can resume deterministically; do not use this
function for passwords or other secrets. The existing `input "Prompt"` statement is different: it
creates an external/manual workflow input gate and does not read from the action terminal.

PTY-backed actions such as `console.run({ interactive: true, ... })` can expose their terminal
transcript in a run's selected action **Step Output** panel. A program can also tell Runinator when
it is blocked on human input. This is explicit rather than prompt detection: write an OSC frame
containing base64url JSON to the terminal, then emit the matching acceptance frame only after the
program has validated the submitted input:

```text
ESC ] 777 ; runinator ; BASE64URL({"version":1,"event":"input_required","request_id":"login","prompt":"Enter the one-time code"}) ESC \
ESC ] 777 ; runinator ; BASE64URL({"version":1,"event":"input_accepted","request_id":"login"}) ESC \
```

Runinator removes valid frames from the visible transcript, durably records the prompt lifecycle,
marks the action `input_required`, and returns it to `running` after the matching acceptance. The
action's normal wall-clock timeout continues throughout the wait. Request ids are 1–128 bytes,
prompts are at most 8 KiB, and decoded payloads are at most 16 KiB. Prompt text is durable and must
not contain secrets; actual keystrokes remain ephemeral, although the child terminal may echo them.

Python:

```python
import base64, json, sys

def runinator_terminal(event, request_id, prompt=None):
    payload = {"version": 1, "event": event, "request_id": request_id}
    if prompt is not None:
        payload["prompt"] = prompt
    encoded = base64.urlsafe_b64encode(json.dumps(payload).encode()).rstrip(b"=").decode()
    sys.stdout.write(f"\x1b]777;runinator;{encoded}\x1b\\")
    sys.stdout.flush()
```

Node:

```javascript
function runinatorTerminal(event, requestId, prompt) {
  const payload = { version: 1, event, request_id: requestId };
  if (prompt !== undefined) payload.prompt = prompt;
  const encoded = Buffer.from(JSON.stringify(payload)).toString("base64url");
  process.stdout.write(`\x1b]777;runinator;${encoded}\x1b\\`);
}
```

POSIX shell (using Python for safe JSON/base64 encoding):

```bash
runinator_terminal() {
  python3 -c 'import base64,json,sys; p={"version":1,"event":sys.argv[1],"request_id":sys.argv[2]}; p.update({"prompt":sys.argv[3]}) if len(sys.argv)>3 else None; e=base64.urlsafe_b64encode(json.dumps(p).encode()).rstrip(b"=").decode(); print("\033]777;runinator;"+e+"\033\\",end="",flush=True)' "$@"
}

runinator_terminal input_required login "Enter the one-time code"
read -r code
# validate $code first
runinator_terminal input_accepted login
```

Live execution uses a frozen VM module and persisted continuations. A linear run starts with one
continuation; `parallel`, `race`, and concurrent `map` work create independently schedulable child
continuations. The engine leases runnable continuations with bounded concurrency (default 16 per
instance), and the module source map projects each instruction pointer back to the node shown in run
details. Provider calls, timers, approvals, signals, child runs, and other asynchronous boundaries
are durable effects. Settling an effect makes its waiting continuation runnable again; retrying an
effect leaves that continuation parked. Branch/body/item blocks return through their compiled
`join`, `try`, `map`, or `race` instructions rather than a reducer rediscovering graph ancestry.

Dry runs are intentionally different: the simulator publishes no effects and does not exercise
continuation leases, broker delivery, retries, timer wakes, or transactional settlement. Use a real
run to validate those behaviors.
