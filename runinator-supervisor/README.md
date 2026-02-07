# runinator-supervisor

Small PM2-style process monitor for local Runinator development.

## CLI

```bash
runinator-supervisor --config runinator-supervisor.json start
runinator-supervisor --config runinator-supervisor.json start --foreground
runinator-supervisor --config runinator-supervisor.json status
runinator-supervisor --config runinator-supervisor.json status --watch
runinator-supervisor --config runinator-supervisor.json stop
```

## Config shape

`runinator-supervisor.json`:

```json
{
  "state_dir": ".runinator-supervisor",
  "shutdown_timeout_secs": 12,
  "restart_delay_ms": 2000,
  "processes": [
    {
      "name": "worker",
      "command": "./target/debug/runinator-worker",
      "args": ["--broker-endpoint", "http://127.0.0.1:7070/"],
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

## Runtime files

- `<state_dir>/supervisor.pid`
- `<state_dir>/state.json`
- `<state_dir>/supervisor.log`
- `<state_dir>/logs/<process>.log`
