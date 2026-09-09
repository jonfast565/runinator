# GitHub clone workspace pack

This pack creates `runinator.tools.github.clone_to_workspace`. It uses the published
`github-default` execution profile on a Kubernetes worker to clone a GitHub repository into a
durable Runinator workspace. It captures the clone's commit and branch in the effect record before
the workflow checkpoint saves the workspace as an immutable version.

The profile must be published and ready before the workflow starts:

```bash
runinatorctl --api-base-url http://127.0.0.1:8081/ execution-profiles status \
  4759a238-939d-46a6-a159-6f1ad6b56aa9
```

Apply the pack and run it with the GitHub organization, repository name, and a durable workspace
key. The example clones this repository into `runinator-source`:

```bash
runinatorctl --api-base-url http://127.0.0.1:8081/ workflows apply packs/github-clone-workspace
runinatorctl --api-base-url http://127.0.0.1:8081/ workflows run \
  runinator.tools.github.clone_to_workspace \
  --param org=jonfast565 \
  --param repo_name=runinator \
  --param workspace_name=runinator-source
```

Inspect the revision effect and retained workspace versions in Command Center or with
`runinatorctl runs show` and `runinatorctl workspaces` after the run.

Run the offline workflow tests without contacting GitHub:

```bash
runinatorctl workflows test packs/github-clone-workspace
```
