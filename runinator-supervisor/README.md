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

`status --watch` opens the interactive supervisor dashboard when run from a terminal. It shows
every managed process in a selectable table, with `running` in green, transitional states such as
`starting` and `backoff` in yellow, failures in red, and intentionally inactive processes in gray.
The two rolling one-minute graphs show the healthy-process percentage and restart events observed
since the dashboard opened. Use `↑`/`↓` (or `j`/`k`) to inspect a process, and `q`/`Esc` to close
the monitor. `start --foreground` uses the same dashboard; there `q`/`Esc` gracefully stops the
supervisor and its children. Piped output keeps the existing plain table refresh.

### Dynamic processes

A running supervisor can add, start, stop, and remove processes on the fly. These commands
drop a request into the control queue (`<state_dir>/control`), which the running daemon drains
each tick. This is the mechanism the web service's on-demand node provisioner uses to spin up
worker/waker nodes.

```bash
runinator-supervisor process add --name worker-2 --command ./target/debug/runinator-worker \
  --arg --broker-backend --arg tcp --env RUNINATOR_LOG=info
runinator-supervisor process start worker-2
runinator-supervisor process stop worker-2
runinator-supervisor process remove worker-2
```

Manually-stopped processes are not auto-restarted; `process start` resumes them and re-arms the
crash-restart policy.

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
        "RUNINATOR_LOG": "info"
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
- `<state_dir>/control/` — dynamic add/start/stop/remove command queue

The supervisor prunes inactive process logs at startup and once per minute. The default retention
limits are seven days, 200 total files, and 512 MiB. Configure them with the optional
`log_retention.max_age_days`, `log_retention.max_files`, and `log_retention.max_bytes` fields; `0`
disables an individual limit. An active process's current log is always protected from deletion.

The repository's local supervisor config runs `runinatorctl workflows apply`
once per configured pack on startup. The checked-in config imports
`packs/hello-world` and `packs/creds-sync`, compiling their unified `.rrx`
sources during import. It also starts `runinator-adapter-host` for inbound
orchestration adapters and a headless desktop agent for desktop-routed work.
The `creds-sync` runs park then fail locally without a usable desktop session
and its local credentials. The config passes `--advertise-host 127.0.0.1` to the web service,
waker, and worker, plus stable local instance ids for the waker and worker, so
the replicas view shows host/IP/version data instead of blank fields after a
restart. The local web-service command runs `runinator-bootstrap` first, then
execs `runinator-ws`; the checked-in config passes
`RUNINATOR_AUTH_BOOTSTRAP_ADMIN=admin:admin` into that bootstrap step so the
admin account is seeded into an empty local database on first start without
enabling auth by default. It also seeds a dev-only bootstrap service API key
and passes that key to the worker, desktop agent, and one-shot pack imports so
the same checked-in config works against both auth-disabled and auth-enabled
local web-service runs.
