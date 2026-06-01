# runinator-supervisor

Small PM2-style process monitor for local Runinator development.

## CLI

```bash
runinator-supervisor --config runinator-supervisor.json start
runinator-supervisor --config runinator-supervisor.json start --foreground
runinator-supervisor --config runinator-supervisor.json restart
runinator-supervisor --config runinator-supervisor.json restart --foreground
runinator-supervisor --config runinator-supervisor.json status
runinator-supervisor --config runinator-supervisor.json status --watch
runinator-supervisor --config runinator-supervisor.json stop
```

## Config shape

`runinator-supervisor.json`:

```json
{
  "shutdown_timeout_secs": 12,
  "restart_delay_ms": 2000,
  "processes": [
    {
      "name": "worker",
      "command": "./target/debug/runinator-worker",
      "args": ["--broker-backend", "tcp", "--broker-endpoint", "127.0.0.1:7070"],
      "cwd": ".",
      "env": {
        "RUST_LOG": "info"
      },
      "autostart": true,
      "restart_on_failure": true,
      "max_restarts_per_minute": 10
    }
  ]
}
```

The broker process selects its serving protocol with
`RUNINATOR_BROKER_TRANSPORT=tcp|http`. Use `host:port` broker endpoints for
TCP clients and `http://host:port/` broker endpoints for HTTP clients.
Kafka and RabbitMQ are direct service backends, not supervisor-managed broker
transports: build the waker, worker, and web service with `--features kafka`
or `--features rabbitmq` and set their `--broker-backend` plus topic/queue
flags.

## Runtime files

When `state_dir` is omitted, supervisor state defaults to
`~/.runinator/supervisor`.

- `<state_dir>/supervisor.pid`
- `<state_dir>/state.json`
- `<state_dir>/supervisor.log`
- `<state_dir>/logs/<process>.log`

The repository's local supervisor config runs the importer once on startup. When
started without an explicit workflow path, the importer uses its normal app-data
default at `~/.runinator/workflows/sdlc.wdlp`. The checked-in supervisor config
uses `packs/sdlc/sdlc.wdlp`, which compiles the referenced `.wdl` files during
import.
