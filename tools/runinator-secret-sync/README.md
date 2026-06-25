# runinator-secret-sync

A host helper that keeps credentials fresh inside a Kubernetes cluster. It is
**fully config-driven** and has no built-in knowledge of any particular
credential — Claude, AWS SSO, GitHub tokens, etc. are just entries in a JSON
spec. It authenticates to the cluster through your local kubeconfig (so **EKS
exec-auth works transparently**) and is never containerized; it runs on a
workstation that can perform any interactive refresh and reach the cluster.

## Model

A config is a list of independent **jobs**. Each job:

1. optionally **refreshes** — runs a command `run`, *unless* a probe command
   `unless` already exits zero (e.g. run `aws sso login` unless
   `aws sts get-caller-identity` succeeds);
2. reads a **source** into a bundle of named blobs:
   - `command` — run argv, capture stdout as one blob,
   - `file` — read one file,
   - `dir` — read every file matching `glob` (default `*`), keyed by file name;
3. reconciles the bundle into one or more **sinks**:
   - `kube-secret` — create/update a namespaced Secret,
   - `file` — write a single-blob bundle to a path (`0600`).

Reconciliation is idempotent: each pass compares the desired bundle against the
live Secret/file, so rotation, drift, and a deleted target are handled the same
way. The secret bytes are never logged — only a short fingerprint.

## Config

See [`secret-sync.example.json`](./secret-sync.example.json). A `command` source
yields a single unnamed blob, so its `kube-secret` sink must set `key` (the data
key); a `dir` source names blobs by file, so `key` is unused. `~` expands to the
home directory in any `path`.

```json
{
  "namespace": "runinator",
  "jobs": [
    {
      "name": "claude",
      "source": { "type": "command", "run": ["keychain-export", "--service", "Claude Code-credentials", "--quiet"] },
      "sinks": [
        { "type": "kube-secret", "name": "claude-credentials", "key": ".credentials.json" },
        { "type": "file", "path": "~/.claude/.credentials.json" }
      ]
    },
    {
      "name": "aws-sso",
      "refresh": {
        "unless": ["aws", "sts", "get-caller-identity", "--profile", "runinator", "--no-cli-pager"],
        "run": ["aws", "sso", "login", "--profile", "runinator"]
      },
      "source": { "type": "dir", "path": "~/.aws/sso/cache", "glob": "*.json" },
      "sinks": [ { "type": "kube-secret", "name": "aws-sso-cache" } ]
    }
  ]
}
```

## Build & run

```sh
go build -o bin/runinator-secret-sync ./...

# watch, syncing every 60s:
bin/runinator-secret-sync --config secret-sync.json

# one-shot preview (writes nothing):
bin/runinator-secret-sync --config secret-sync.json --once --dry-run
```

Or use the wrapper, which builds `keychain-export` + this engine and puts the
helper on PATH so a config can call it by name:

```sh
scripts/sync-secrets.sh --once --dry-run
scripts/sync-secrets.sh --config my-spec.json --interval 5m
```

Flags: `--config` (default `secret-sync.json`), `--interval`, `--once`,
`--dry-run`, `--kubeconfig`, `--context`, `--namespace` (override every
kube-secret sink's namespace). A kube client is only built when at least one
`kube-secret` sink exists, so file-only configs need no cluster access.

## How it pairs with the cluster

`deploy/k8s/components/rotated-creds` mounts the pushed Secrets into the worker
(`aws-sso-cache` whole-dir at `~/.aws/sso/cache` for live SSO rotation;
`claude-credentials` via subPath at `~/.claude/.credentials.json`). The Secret
names there must match the `kube-secret` sink names in your config.

## RBAC

Running as your own kubeconfig identity needs no setup. To run as a dedicated
ServiceAccount (e.g. from automation), apply
`deploy/k8s/components/rotated-creds/sync-rbac.yaml` and point `--kubeconfig` at a
kubeconfig built from its token Secret.

## Caveats

- A `refresh.run` like `aws sso login` is interactive — run the engine where a
  browser/device prompt can be completed; pick `--interval` shorter than the SSO
  session lifetime.
- The `keychain-export` helper needs its one-time Keychain "Always Allow" granted
  interactively before unattended runs.
