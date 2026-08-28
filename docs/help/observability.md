# Observability

Use this guide to collect logs, metrics, and traces locally or in Kubernetes, and to inspect dead letters and audit events.

## Observability

Every service binary (`ws`, `engine-worker`, `worker`, `waker`) emits structured logs to stdout and
a log file via `tracing`, filtered by `RUNINATOR_LOG` (an `EnvFilter` directive,
default `info`). The web service additionally exposes Prometheus metrics at
`/metrics`.

The `runinator-desktop-agent` tray app honors the same `RUNINATOR_LOG` directive at
startup and additionally renders those `tracing` records into its in-app log console,
where a **Log → Level** dropdown changes the level live (no restart) and persists it.

OpenTelemetry export is **off by default and turns on purely from the standard
`OTEL_*` environment variables** — no CLI flags or config-file options. When
`OTEL_EXPORTER_OTLP_ENDPOINT` (or a signal-specific
`OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT`) is set, each binary stands up
OTLP exporters for **traces, metrics, and logs** over OTLP HTTP/protobuf;
`OTEL_SDK_DISABLED=true` forces it off. The service name defaults to the binary
(e.g. `Runinator Web Service`) and is overridable with `OTEL_SERVICE_NAME` /
`OTEL_RESOURCE_ATTRIBUTES`.

Trace context propagates across hops using W3C `traceparent`: inbound HTTP requests
to the web service continue the caller's trace, and the graph runtime stamps the active
context onto each `EffectCommand` so a worker's execution span links back to the
dispatching trace. Prometheus `/metrics` remains available alongside OTLP metrics.

Each service and the broker emit runtime metrics over OTLP (and, for the web
service, also on Prometheus `/metrics`):

- **Engine** (`runinator_engine_*`, emitted by `ws` when embedded or by `engine-worker`):
  effect/result processing, trigger, maintenance, and VM-drive metrics. The VM's
  `runinator_vm_drive_duration_ms` histogram measures continuation claim-and-advance batches.
- **Worker** (`runinator_worker_*`): `actions_received_total`, `actions_completed_total`
  and the `action_duration_ms` histogram (both split by `outcome`),
  `actions_duplicate_total`, `actions_in_flight` (gauge), `control_commands_total`
  (by `kind`), and `secret_resolution_failures_total`.
- **Waker** (`runinator_waker_*`): `wakes_{received,driven,requeued}_total`,
  `drive_failures_total`, `broker_heartbeats_total`, `broker_heartbeat_failures_total`, and the
  `wake_lead_ms` histogram (scheduling lead/lag at receipt).
- **VM** (`runinator_vm_*`): `continuations_driven_total` split by the bounded
  `outcome` label (`yielded`, `forked`, `joined`, `completed`, `failed`), the
  `drive_duration_ms` histogram for claim-and-advance batches, and
  `driver_failures_total`.
- **Broker** (`runinator_broker_*`, emitted by every service): `operations_total` and
  the `operation_duration_ms` histogram, tagged with `backend` (in-memory/http/tcp/
  kafka/rabbitmq), `channel`, `op`, and (for the counter) `outcome`.

```bash
# point all binaries at a local OpenTelemetry Collector (OTLP/HTTP on :4318)
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
cargo run -p runinator-supervisor -- start
```

For the checked-in local supervisor flow, prefer the one-command helper:

```bash
bash scripts/run-local.sh observe
```

It starts `deploy/local-observability/compose.yaml` with:

- OpenTelemetry Collector receiving OTLP HTTP on `http://127.0.0.1:4318`
  and OTLP gRPC on `127.0.0.1:4317`.
- Jaeger at `http://127.0.0.1:16686` for traces.
- Prometheus at `http://127.0.0.1:9090` scraping the collector's re-exported
  OTLP metrics on `otel-collector:8889` plus collector self-metrics on `:8888`.
- Loki at `http://127.0.0.1:3100` receiving the collector's logs signal via
  OTLP, so structured fields the binaries set (`trace_id`, `run_id`,
  `error_code`, ...) are queryable with LogQL instead of only living in
  stdout/log files.
- Grafana at `http://127.0.0.1:3000` (anonymous admin) with Loki, Prometheus,
  and Jaeger pre-provisioned as datasources — the natural place to query logs
  and click a `trace_id` through to its Jaeger trace.

After the stack starts, run `bash scripts/run-local.sh smoke-sync` or drive a
workflow through the UI/CLI, then inspect traces in Jaeger, metrics in
Prometheus, and logs in Grafana (or `loki`, e.g. via `logcli`). Use
`bash scripts/run-local.sh observability-logs --lines 120` to inspect
collector/exporter output, and use `bash scripts/run-local.sh
observability-stop` to stop the local observability containers.

**In Kubernetes**, the `components/observability` kustomize component deploys an
OpenTelemetry Collector, Jaeger (trace UI), Prometheus (scrapes the collector),
Loki (durable/queryable log store), and Grafana (dashboards over Prometheus +
Jaeger + Loki), and points the services at the collector. It is enabled in the
`local` overlay by default; add `../../components/observability` to another
overlay's `components:` list to turn it on there (and remove it to turn otel
back off). After deploying:

```bash
# dashboards + logs — open Grafana at http://localhost:3000 (anonymous admin; "Runinator
# Overview" dashboard is provisioned, with Loki + Prometheus + Jaeger datasources wired up)
bash scripts/port-forward-grafana.sh   # or: kubectl -n runinator port-forward svc/runinator-grafana 3000:3000
# traces — open the Jaeger UI at http://localhost:16686
kubectl -n runinator port-forward svc/runinator-jaeger 16686:16686
# raw metrics — the Prometheus UI / API at http://localhost:9090
kubectl -n runinator port-forward svc/runinator-prometheus 9090:9090
# raw logql — the Loki API at http://localhost:3100
kubectl -n runinator port-forward svc/runinator-loki 3100:3100
# a copy of every signal — the collector's debug exporter
kubectl -n runinator logs deploy/runinator-otel-collector
```

Grafana's anonymous-admin login is for convenient local viewing; lock it down (set
a real admin password and disable anonymous access) before using it on a shared
cluster.

### Dead letters and audit log

Poison messages are no longer dropped silently. When a result or ingress event
cannot be applied and is given up on, the engine persists a `dead_letters`
row before acking, so failed messages have a durable record. Auth and sensitive
operations (login success/failure, authorization denials) are recorded to an
`audit_log` table. Both are exposed as admin-only endpoints (`GET /dead_letters`,
`GET /audit_log`, in the OpenAPI spec) and surfaced in the command center as
admin-gated **Dead Letters** and **Audit Log** views.
