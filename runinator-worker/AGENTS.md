# AGENTS.md

Guidance for `runinator-worker`, `runinator-desktop-agent`, `runinator-plugin`, and provider crates.

## Ownership

`runinator-worker` consumes provider effects, resolves providers/plugins, executes actions, and
publishes results. It self-publishes built-in provider metadata at startup. It does not calculate
schedules or write workflow state directly; durable orchestration belongs to the engine.

`runinator-desktop-agent` is a standalone, exclusive desktop `WorkerRuntime` host with tray UI,
relay support, and sandboxed local-files access. Reusable execution behavior stays in the worker;
never move its lifecycle into the command center.

## Delivery and Artifact Invariants

- Acknowledge a broker delivery only after required terminal results/artifacts are durable in the
  broker or local result outbox.
- Outbox fallback requires fsync before acknowledgement, draining before the action loop starts,
  and a hard size/count cap that puts the agent into draining state. Log chunks remain bounded
  best-effort data and are not buffered; `idempotency_key` protects failures outside the envelope.
- Publish outputs, logs, artifact metadata, and node status as result events for the engine. Never
  write them directly to the database.
- Relocate provider-produced artifact bytes through `POST /artifacts/content` before publishing the
  artifact event. The upload records no row; the engine's event path records it once.
- Enforce the worker-side action timeout in-process. The engine's later deadline wake is a backstop,
  not a replacement or a reason to retry locally.
- Keep blocking provider/plugin calls behind `spawn_blocking` or equivalent isolation.

## Provider and Plugin Boundaries

- Provider resolution stays in `runinator-worker`; dynamic loading and FFI safety wrappers stay in
  `runinator-plugin`. Treat `runinator_marker`, `name`, `call_service`, and metadata ABI symbols as
  public contracts.
- A new provider is a separate `runinator-provider-<name>` crate. Provider metadata stays beside
  executable behavior through `Provider::metadata()` or the plugin metadata ABI; do not duplicate
  it in workflow/provider packs.
- Providers perform outbound actions. They do not schedule, persist, or implement inbound adapter
  polling/webhooks. Prefer a maintained client library over hand-written third-party API semantics.
- Dynamic plugins are host-only. Static musl container binaries cannot `dlopen`; Kubernetes workers
  contain only compiled-in providers. Do not add plugin directories, `--dll-path`, or staging init
  containers to images/manifests. The desktop agent, `xtask local up`, and release bundles ship
  plugins. A provider required in Kubernetes must be compiled in.
- `action_name`, `action_function`, and `action_configuration` remain compatible with existing
  import and execution paths.
- The authenticated desktop relay is intentionally narrower than broker access: effect,
  effect-result, control, and agent operations, plus `PublishIngress` only for
  `AgentDirectiveResult`. Never allow an agent principal to publish arbitrary ingress commands.

## Where to Start

- Worker runtime/effect loop/provider resolution: `src/`.
- Plugin loading/ABI: `../runinator-plugin/src/`.
- Desktop lifecycle: `../runinator-desktop-agent/src/`.
- Provider metadata/action behavior: the relevant `../runinator-provider-*/src/`.
- Container execution: `../runinator-sandbox/AGENTS.md`.

## Verification

```bash
cargo check -p runinator-worker
cargo test -p runinator-worker
```

Provider/plugin changes require the focused provider/plugin tests. Delivery, timeout, or shared
payload changes also require engine and broker checks; desktop-only changes use the desktop build
and do not trigger a cluster deployment unless shared cluster-owned code changed.
