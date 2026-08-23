# Runinator Architecture

Runinator is a Rust workspace for authoring, scheduling, and executing workflows across a small local or distributed runtime. Workflows are durable state machines: the graph interpreter records its progress, dispatches side effects through a broker, and resumes only when a result or scheduled wake returns.

The design keeps three concerns separate:

- **Authoring and control plane** — the CLI and command center compile and manage workflows through the authenticated HTTP/WebSocket service.
- **Durable orchestration** — the engine drives the workflow VM, persists state, dispatches effects, consumes results, and reconciles triggers and maintenance work.
- **Execution plane** — workers and the waker independently consume broker work. They never write directly to the database.

## System overview

```mermaid
flowchart LR
    subgraph Authors["Authors and operators"]
        CC["Command center\nTauri + Vue"]
        CLI["runinatorctl\nCLI, console, MCP"]
        RRX[".rrx workflow packs"]
    end

    subgraph Authoring["Authoring and API"]
        REXRAP["REXRAP language pipeline\nsyntax → sema → codegen"]
        PACK["runinator-pack\ncompiled pack ZIP"]
        WS["runinator-ws\nHTTP, WebSockets, API assembly"]
        HANDLERS["ws handler crates\nidentity · authoring · runtime"]
        GATE["ws middleware\nauth · authz · rate limits · overload"]
    end

    subgraph Orchestration["Durable orchestration"]
        ENGINE["runinator-engine\nrepository and background loops"]
        RUNTIME["runinator-runtime\ncontinuation-driven WorkflowMachine"]
        GRAPH["runinator-workflows\nvalidated workflow graph"]
        COMPUTE["runinator-compute\nexpressions and compute VM"]
    end

    subgraph Transport["Broker contract and selected transport"]
        BROKER["runinator-broker-core + runinator-broker\nBroker trait; TCP, HTTP, WS, Kafka, RabbitMQ"]
    end

    subgraph Execution["Independent execution processes"]
        WORKER["runinator-worker\nproviders and plugins"]
        WAKER["runinator-waker\ntimer relay"]
        AGENT["runinator-desktop-agent\nexclusive desktop worker"]
    end

    subgraph Data["Durable data and artifacts"]
        STORE["runinator-store\npersistence traits"]
        DB["runinator-database\nSQLite, Postgres, MySQL"]
        BLOB["BlobStore\nlocal filesystem or S3-compatible"]
    end

    RRX --> REXRAP --> PACK
    CC --> WS
    CLI --> WS
    CLI --> PACK
    PACK -->|"POST /packs/import"| WS
    WS --> GATE --> HANDLERS
    HANDLERS --> ENGINE
    WS -. "default host" .-> ENGINE

    ENGINE --> RUNTIME
    RUNTIME --> GRAPH
    RUNTIME --> COMPUTE
    ENGINE --> STORE --> DB
    ENGINE --> BLOB

    ENGINE -->|"effect"| BROKER --> WORKER
    WORKER -->|"effect_result"| BROKER --> ENGINE

    ENGINE -->|"wake"| BROKER --> WAKER
    WAKER -->|"ingress: SettleEffect"| BROKER --> ENGINE

    ENGINE -->|"agent"| BROKER --> AGENT
    AGENT -->|"ingress: directive result"| BROKER --> ENGINE

    ENGINE -->|"control"| BROKER --> WORKER
    AGENT -. "reuses worker runtime" .-> WORKER
```

`runinator-ws` embeds the engine in the default topology. Deployments that need to scale HTTP and background work independently instead run `runinator-engine-worker` against the same database and broker; it hosts the same `runinator-engine` library rather than introducing another execution path.

## Major boundaries

### API and user-facing clients

`runinator-ws` is the HTTP/WebSocket assembly crate. Its smaller companion crates divide responsibility deliberately:

- `runinator-ws-core` owns wire models, response envelopes, UI events, and OpenAPI vocabulary.
- `runinator-ws-middleware` owns authentication, authorization, rate limiting, and overload protection.
- `runinator-ws-identity`, `runinator-ws-authoring`, and `runinator-ws-runtime` own their domain handlers and route registrations.

The Tauri command center presents the web service; it does not host a worker or execute provider actions. `runinatorctl` is the terminal control client, and its console and MCP server dispatch through the same command surface as its normal command-line interface.

### Workflow definition and evaluation

REXRAP is the authored workflow language. The language family is split by compile stage: `runinator-rexrap-syntax` parses and formats, `runinator-rexrap-sema` resolves and type-checks, and `runinator-rexrap-codegen` lowers to or decompiles from the workflow model. `runinator-rexrap` is the public facade and unified `.rrx` container front end; `runinator-rexrap-ide` adds editor completion and hover without affecting compilation.

`runinator-workflows` validates graphs, node parameters, types, and graph invariants. It builds on `runinator-compute`, which evaluates references, templates, conditions, compute programs, and intrinsic functions without knowing about workflow graphs. `runinator-runtime` runs only validated workflow graphs.

Pack compilation is client-side. `runinator-pack` compiles `.rrx` sources into a workflow bundle, `runinator-pack-wire` defines the compiled ZIP wire format, and the web service imports the compiled JSON without recompiling source.

### Durable orchestration

`runinator-runtime` contains the continuation-driven `WorkflowMachine`. A run can have multiple durable cursors—one for each live thread of control—so parallel and race branches are independent fibers rather than a single mutable position. The runtime asks a `WorkflowHost` to record state and perform external effects; it has no HTTP server, concrete broker, or SQL implementation.

`runinator-engine` is that runtime's durable orchestrator. Its loops drive cursors, fire triggers, drain durable effect dispatches, consume effect results and ingress commands, handle effect retry/deadline policy, publish wakes and agent directives, and run repository maintenance. Its repository layer coordinates persistence through trait contracts rather than placing persistence logic in the web handlers.

### Brokered execution

`runinator-broker-core` defines the backend-neutral `Broker` trait, delivery wrappers, and in-memory implementation. `runinator-broker` adds the selectable TCP, HTTP, WebSocket, Kafka, and RabbitMQ transports. The serializable commands and results that cross those boundaries live in `runinator-comm`. Components that only publish or receive through `dyn Broker` depend on the core crate; binaries that construct a backend use the transport crate.

The core execution message paths are:

- The engine publishes provider `effect` commands; workers resolve a provider or plugin, execute it, and publish an `effect_result`.
- The engine turns a known future completion into a `wake`. The waker waits and relays the prebuilt result on `ingress` as `SettleEffect`; the engine's normal result-settlement path then processes it.
- API-originated control travels through the engine and the `control` channel. Durable desktop/replica directives use the `agent` channel, and their replies return through `ingress`.

The `effect`, `control`, and `agent` paths are target-routed. `events` is the exception: it is fan-out so every web-service replica can update its connected WebSocket clients. There is intentionally no worker-to-waker or waker-to-worker channel.

### Persistence and artifacts

`runinator-models` and `runinator-comm` are the shared domain and wire-contract foundations. `runinator-store` declares persistence roles and the focused `RuntimeStore` use-case trait used by the graph runtime. `runinator-database` provides their SQLite, Postgres, and MySQL implementations and owns SQL mapping. This keeps database behavior out of the HTTP handler and runtime crates.

Workflow and task state, cursors, effect dispatches, triggers, and metadata are durable database records. Artifact bytes are different: `runinator-blob-core` defines `BlobStore`, while `runinator-blob` supplies the S3-compatible client/server transport. The engine's artifact-storage boundary writes bytes to the object store and persists their `blob://` references; workers upload produced bytes through the API before reporting artifact events.

## Runtime lifecycle

1. An operator imports a compiled pack or starts a run through the command center or `runinatorctl`.
2. The web service authenticates and authorizes the request, then delegates its durable work to the engine.
3. The engine loads the definition and run state, then drives `WorkflowMachine` until it reaches a terminal state or parks a cursor on an effect, external signal, child run, or join.
4. Provider work is persisted as a dispatch and published to `effect`. A worker executes it and returns an `effect_result`; the engine applies the result, records run events and artifacts, and drives the waiting cursor again.
5. Timed infrastructure effects do not occupy an engine task. The engine publishes a due-time `wake`; the stateless waker relays the already-built settlement back through `ingress` at that time.
6. When the last real cursor retires, the runtime transitions the run to its terminal status. Results, logs, events, and artifacts remain durable and available through the API.

## Extension guide

Keep dependency direction from service and execution crates toward shared contracts:

- Add a workflow node in `runinator-workflows`, then teach the runtime its behavior; do not encode graph rules in a web handler or provider.
- Add persistence operations to the owning `runinator-store` role, with the implementation and database mapping in `runinator-database`.
- Add shared broker payloads to `runinator-comm` and implement them across every relevant broker backend and consumer.
- Add a provider as a dedicated `runinator-provider-<name>` crate, resolved by `runinator-worker`; providers do not schedule work or write directly to the database.
- Keep reusable container execution in `runinator-sandbox`, provider loading in `runinator-plugin`, and binary startup/configuration in the platform and bootstrap crates.

This separation lets the local supervisor stack use one embedded engine and an in-memory-style development setup, while production can independently scale web-service, engine, worker, waker, broker, database, and object-store replicas without changing workflow semantics.
