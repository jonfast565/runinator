# Command center

Use this guide to run the Tauri command center against a local stack or a Kubernetes deployment, and to understand its role as a control-plane client.

## Build Command Center

`runinator-command-center` is a Tauri client. Run it against the local stack with:

```bash
bash scripts/run-local.sh ui
```

The default local stack advertises and serves the API on `127.0.0.1:8080`.
For Kubernetes, gossip is disabled and the web service is available through the
`runinator-ws` Service instead. Use the K8s UI launcher to create the
port-forward and pass the concrete API URL:

```bash
bash scripts/run-k8s.sh ui
```

The command center checks `RUNINATOR_COMMAND_CENTER_SERVICE_URL`,
`RUNINATOR_SERVICE_URL`, then `WS_API_BASE_URL` before falling back to gossip.
It is a pure client and does not execute workflow actions itself; use the
desktop agent below to run actions on your own machine.

## Workflow run timing

The Workflow Runs detail view includes an analog elapsed-time clock. Its hour, minute, and second hands start at 12 and tick each second while the run is active, including waits and pauses. After the run finishes, the clock stays visible at the recorded final duration. The numeric hours/minutes/seconds display preserves total hours beyond one dial revolution. Queued runs stay at zero until they start; completed runs without timing data show “Timing unavailable.”

## Timeline categories

The backend labels workflow run events by semantic category. Authored node entry, retry, and failure
activity is **User**, while runtime-managed workspace, branch-fork, and interrupt lifecycle activity
is **System**. Category chips above the step list control visibility in both the step list and
proportional timeline, and the choice is stored in the browser. When the backend adds another
non-empty `timeline_category`, the command center humanizes it and gives it the same visibility
control automatically. Internal continuation transitions and duplicate effect request/settlement
boundaries remain available in the VM journal without becoming separate timeline rows.
