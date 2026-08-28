# Runinator

Runinator is a Rust workspace for scheduling and executing durable workflows across a small local or distributed runtime.

Start a local stack with:

    bash scripts/run-local.sh start

Then use the task-oriented guides below. The previous single-file README has been split so setup, authoring, deployment, and operational details are easier to find and maintain.

## Help guides

### Start and operate

- [Local development and runtime](docs/help/local-development.md) — prerequisites, local startup, authentication, runtime topology, transports, dashboards, and the cross-platform local runner.
- [Workflow authoring and import](docs/help/workflow-authoring.md) — REXRAP packs, testing and simulation, control flow, triggers, ingress, notifications, schedules, and settings.
- [Console and MCP](docs/help/console-and-mcp.md) — terminal and browser consoles plus Model Context Protocol integration.
- [Packaged functions](docs/help/packaged-functions.md) — package, publish, invoke, and operate immutable function code.
- [Observability](docs/help/observability.md) — logs, metrics, traces, local telemetry, and incident records.

### Deploy and use clients

- [Kubernetes deployment](docs/help/kubernetes.md) — cluster topology, object storage, image builds, bootstrap, key rotation, and rollout.
- [Command center](docs/help/command-center.md) — run the Tauri control-plane client locally or against Kubernetes.
- [Desktop agent](docs/help/desktop-agent.md) — the exclusive desktop worker, connectivity, discovery, relay, and fleet behavior.
- [Releases and macOS packaging](docs/help/releases-and-packaging.md) — workspace versioning and macOS app bundles.
- [Verification](docs/help/verification.md) — workflow-pack checks, local sync and smoke helpers, and end-to-end validation.

See the complete [help-guide index](docs/help/README.md) for the same directory organized by task.

## Reference

- [Architecture](docs/architecture.md)
- [Permissions](docs/permissions.md)
- [Server settings](docs/server-settings.md)
- [Jargon](docs/JARGON.md)
- [LLM map](docs/llm-map.md)
