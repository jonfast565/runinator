# runinator
Run/schedule the whole world!

## Local Process Monitor (runinator-supervisor)

A lightweight PM2-style supervisor is included to run the local Runinator stack without Docker or Kubernetes.

### 1. Build the binaries

```bash
cargo build --workspace
```

### 2. Start the supervisor (daemon mode)

```bash
cargo run -p runinator-supervisor -- start
```

This uses `runinator-supervisor.json` to start:
- `runinator-broker`
- `runinator-ws`
- `runinator-scheduler`
- `runinator-worker`
- `runinator-importer`

Failed processes are automatically restarted.

### 3. Check process status

```bash
cargo run -p runinator-supervisor -- status
```

Watch live:

```bash
cargo run -p runinator-supervisor -- status --watch
```

### 4. Stop everything

```bash
cargo run -p runinator-supervisor -- stop
```

Runtime files/logs are written under `.runinator-supervisor/`.

The local build script now uses the supervisor automatically when run with local mode and run enabled:

```powershell
./build.ps1 -Mode Local -Run
```
