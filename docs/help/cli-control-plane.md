# CLI control-plane operations

`runinatorctl` exposes the same server-backed recovery and lifecycle operations as Command Center.
All commands accept the global `--json` switch. JSON mode returns the complete API response; binary
downloads require an explicit destination and never write to standard output.

## Workflow lifecycle and recovery

```sh
runinatorctl workflows simulate workflow.json --input-file input.json
runinatorctl workflows contract-impact workflow.json
runinatorctl workflows import-archive compiled-pack.zip --overwrite
runinatorctl workflows enable daily-report
runinatorctl workflows disable daily-report
runinatorctl workflows delete daily-report

runinatorctl runs step RUN_UUID --cursor CONTINUATION_UUID
runinatorctl runs continue RUN_UUID
runinatorctl runs breakpoints set RUN_UUID --breakpoint review --breakpoint publish
runinatorctl runs to-node RUN_UUID CONTINUATION_UUID publish
runinatorctl runs pause-on-failure RUN_UUID true
runinatorctl runs signal RUN_UUID approval_received --json-file signal.json
runinatorctl runs interrupt RUN_UUID operator --json-file interrupt.json
runinatorctl runs resolve-input EFFECT_UUID --json-file answer.json
runinatorctl runs terminal input EFFECT_UUID $'yes\n'
runinatorctl runs terminal resize EFFECT_UUID 120 40
runinatorctl runs terminal close EFFECT_UUID
```

The definition, signal, interrupt, and input files are JSON. Run-control commands retain the
server's stale-cursor and terminal-run protections and return the structured task response.

## Triggers, freezes, and calendars

```sh
runinatorctl triggers show TRIGGER_UUID
runinatorctl triggers apply trigger.json
runinatorctl triggers delete TRIGGER_UUID
runinatorctl freeze update WINDOW_UUID freeze-window.json
runinatorctl freeze calendar subscribe --scope organization --org-id ORG_UUID
runinatorctl freeze calendar unsubscribe SUBSCRIPTION_UUID
runinatorctl freeze calendar download --output schedule.ics --scope user
```

Calendar subscription creation prints the secret-bearing response once. Treat it like a credential.

## Files, artifacts, workspaces, and notebooks

```sh
runinatorctl files list
runinatorctl files upload invoice.csv --path fixtures/invoice.csv --mime text/csv
runinatorctl files download FILE_UUID --output invoice.csv
runinatorctl files archive FILE_UUID
runinatorctl artifacts download --effect EFFECT_UUID --event EVENT_UUID --output artifact.bin
runinatorctl workspaces delete WORKSPACE_UUID --version 3
runinatorctl workspaces delete WORKSPACE_UUID

runinatorctl notebooks sessions list
runinatorctl notebooks sessions create investigation
runinatorctl notebooks sessions rename SESSION_UUID incident-42
runinatorctl notebooks sessions clear SESSION_UUID
runinatorctl notebooks cells create SESSION_UUID cell.rrx --label inspect
runinatorctl notebooks cells update CELL_UUID fixed-cell.rrx
runinatorctl notebooks cells run CELL_UUID
runinatorctl notebooks cells replay CELL_UUID
runinatorctl notebooks cells cancel CELL_UUID
runinatorctl notebooks cells delete CELL_UUID
```

`workspaces delete --version` removes one immutable version; omitting it removes the workspace.
Notebook source is read from a file so multiline REXRAP remains shell-safe and reproducible.

## Notifications, gates, and orchestration recovery

```sh
runinatorctl notifications list --unread
runinatorctl notifications read NOTIFICATION_UUID
runinatorctl notifications read-all
runinatorctl notifications action NOTIFICATION_UUID retry --input action.json
runinatorctl notifications policies apply policy.json
runinatorctl gates list --run-id RUN_UUID
runinatorctl gates open GATE_UUID --reason "operator approved"

runinatorctl orchestrations epochs ORCHESTRATION_UUID
runinatorctl orchestrations commands ORCHESTRATION_UUID
runinatorctl orchestrations evidence ORCHESTRATION_UUID
runinatorctl orchestrations workspaces ORCHESTRATION_UUID
runinatorctl orchestrations operations list ORCHESTRATION_UUID
runinatorctl orchestrations operations resolve ORCHESTRATION_UUID OPERATION_UUID succeeded \
  --reason "receipt verified" --receipt receipt.json
runinatorctl orchestrations debug pause PIPELINE_UUID
runinatorctl orchestrations debug step PIPELINE_UUID
```

Adapter diagnostics include `summaries`, `health`, draft `validate`/`test-draft`, inspection-mode
changes, held-delivery decisions, and paused-delivery release beneath `orchestrations adapters`.

## Ingress and operational evidence

```sh
runinatorctl ingress external list --state held
runinatorctl ingress external configure workflow WORKFLOW_UUID review
runinatorctl ingress external approve INBOX_UUID
runinatorctl ingress external release workflow WORKFLOW_UUID
runinatorctl ingress broker session organization --scope-id ORG_UUID
runinatorctl ingress broker configure organization observe --scope-id ORG_UUID
runinatorctl ingress broker renew organization --scope-id ORG_UUID
runinatorctl ingress messages list --workflow-run RUN_UUID
runinatorctl ingress dead-letters list --channel ingress
runinatorctl audit list --action ingress.approve
runinatorctl records external-items
runinatorctl records events
```

Ingress decisions and orchestration operation resolutions remain subject to backend resource
authorization. The CLI does not infer administrative access or bypass held-record state checks.
