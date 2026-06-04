# Hello world smoke pack

This pack is the smallest checked-in WDL import path for proving a local
ws/waker/worker stack can compile a pack, create a workflow run, dispatch a
console action, and persist the worker result.

Import only:

```bash
runinatorctl workflows apply ./packs/hello-world/hello-world.wdlp
```

Import and execute against a running local stack:

```bash
bash scripts/run-local.sh smoke-sync
```
