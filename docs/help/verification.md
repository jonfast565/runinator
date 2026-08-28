# Verification

Use this guide to validate workflow packs and exercise the local end-to-end runtime checks.

## Verification

For workflow pack import changes, first start the target API: `rexrap check` reads its provider
and packaged-function catalog so strict type checks match the server that will receive the pack.
Then run:

```bash
runinatorctl rexrap check packs/hello-world/hello-world.rrx
runinatorctl rexrap check packs/creds-sync/creds-sync.rrx
runinatorctl rexrap check packs/autonomous-development/autonomous-development.rrx
cargo test -p runinator-ctl
```

To sync the seed file manually against a running local API:

```bash
bash scripts/run-local.sh sync
```

To run the tiny smoke pack against a running local stack:

```bash
bash scripts/run-local.sh smoke-sync
```

To verify rich workflow execution end-to-end against an isolated local stack:

```bash
RUNINATOR_E2E=1 cargo test -p runinator-e2e -- --ignored
```
