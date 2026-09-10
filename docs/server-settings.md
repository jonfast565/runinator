# Server settings

Runinator stores platform-wide operating policy as the encrypted config setting
`server/operational_policy`. The document is typed by `runinator-models::server_settings`, is
validated before it is written, and is edited through `GET`/`PUT /server/settings`.

The response contains both `values` and a `catalog`. The catalog is the single source for the
Command Center form: every entry supplies its compiled default, hard valid bounds, usual operating
range, unit, label, and description. New struct fields use their compiled defaults when an older
stored document is read.

The Command Center exposes the catalog under Settings → Server, grouped into Authentication,
Orchestration, Notifications, Workers, Wakers, Engine Workers, Replicas, and Archiver. Saving writes
the full document atomically. Embedded and standalone engines share a cached snapshot and refresh it
at the configured refresh interval; standalone workers read only the worker subset through
`GET /worker/settings` and gracefully drain and rebuild their action loop when that policy changes.
Desktop agents retain their machine-local settings. Standalone engine workers apply ingress
concurrency from the same refreshed snapshot. The engine attaches the current waker policy to newly
armed wake messages so wakers remain broker-only relays and adopt the policy without calling the API
or settings store.

The current catalog covers:

- authentication refresh-session limits;
- durable claim batch size, trigger/directive/VM/effect polling, dispatch leases, action-deadline
  grace, timer horizon, workspace reconciliation, usage/metrics sampling, settings refresh, and
  synchronous invocation wait/poll timing;
- notification scanning, batch size, default secret-expiry warning, and delivery timeout;
- standalone worker concurrency, shutdown grace, reconnect budget, and settings refresh timing;
- waker concurrency and maximum sleep interval;
- standalone engine-worker ingress concurrency;
- replica stale/reap/delete windows, cleanup cadence, telemetry retention/window, and point limit.

Replica windows also have relational validation: `reap_after_seconds` must exceed
`stale_after_seconds`, and `delete_after_seconds` must exceed `reap_after_seconds`.

## Values intentionally left in code or process configuration

The audit does not turn every numeric literal into mutable policy. These categories remain fixed:

- command-line and environment bootstrap defaults (ports, backend selection, authentication token
  TTLs, and overload/rate-limit options), because they are needed before
  the settings store or HTTP service is available and already have process-level configuration;
- wire/protocol and security bounds (WebSocket frame sizes, S3 multipart limits, SigV4 clock skew,
  enrollment-token maximum TTL, stale broker-message TTL, and protocol versions), because peers or
  security checks must agree on them;
- correctness and resource-safety guards (VM inline-instruction ceiling, pipeline recursion depth,
  rate-limiter pruning threshold, fixed queue capacities, and maximum upload/object sizes), because
  raising them at runtime can invalidate memory or termination guarantees;
- per-request pagination defaults and caps, which are API behavior selected by each caller rather
  than server operating policy;
- per-workflow/action values such as action timeout, retry policy, debounce, approval expiry, and
  gate polling, which are authored into the durable workflow/effect and must not change underneath a
  running execution;
- desktop-agent, broker, and blob-process local settings. Standalone workers, wakers, and engine
  workers preserve their CLI configuration until an administrator first saves the unified policy;
  after that, their respective server-settings sections become authoritative. Wakers receive new
  policy on subsequently armed wake messages, so an idle waker can continue to report its process
  source until it services the first new policy-bearing wake.

The reserved policy row is omitted from the generic credentials list and workflow config type tree.
Generic credential writes, moves, deletes, and pack imports reject that coordinate, so all policy
updates pass through the same validation path.
