# Runinator help guides

Use these task-oriented guides in place of the former monolithic repository README.

## Start and operate Runinator

- [Local development and runtime](local-development.md) — prerequisites, the supervisor stack, authentication, topology, transports, dashboards, and the cross-platform local runner.
- [Workflow authoring and import](workflow-authoring.md) — REXRAP packs, local testing, dry runs,
  control flow, pipelines, correlated orchestration, ingress adapters, notifications, schedules,
  and settings. The crate docs split out the [language reference](../../runinator-rexrap/docs/language-reference.md),
  [runtime model](../../runinator-rexrap/docs/runtime.md), and
  [CLI/pack workflow](../../runinator-rexrap/docs/tooling.md).
- [Console and MCP](console-and-mcp.md) — the terminal and browser consoles, command completion, session behavior, and MCP integration.
- [Packaged functions](packaged-functions.md) — immutable function packages, workflow and HTTP invocation, pack integration, execution, and retention.
- [Observability](observability.md) — logs, metrics, traces, the local telemetry stack, Kubernetes observability, dead letters, and audit logs.

## Deploy and use clients

- [Kubernetes deployment](kubernetes.md) — kustomize overlays, object storage, images, bootstrap, key rotation, local-cluster deployment, and production rollout.
- [Command center](command-center.md) — running the Tauri control-plane client locally or against Kubernetes.
- [Desktop agent](desktop-agent.md) — the exclusive desktop worker, connectivity, discovery, relays, and fleet behavior.
- [Releases and macOS packaging](releases-and-packaging.md) — workspace versioning and macOS app bundles.
- [Verification](verification.md) — pack checks, sync and smoke helpers, and end-to-end validation.

## Reference

- [Architecture](../architecture.md)
- [Permissions](../permissions.md)
- [Server settings](../server-settings.md)
- [Jargon](../JARGON.md)
- [LLM map](../llm-map.md)
