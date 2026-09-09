# GitHub availability test pack

This pack creates `runinator.tests.github.github_availability`. It runs on a Kubernetes worker
with the `github-default` execution profile and calls GitHub's authenticated `user` endpoint
through the GitHub CLI. Its purpose is to prove that credentials collected on the desktop reach a
non-desktop worker and work there. The workflow returns `yes` on success; it returns `no` and
fails when that end-to-end integration cannot run.

The desktop agent only collects and publishes the locally approved profile. The Kubernetes worker
downloads the published profile through the worker API, verifies and stages it in a temporary home
directory, invokes `gh api user`, and deletes that directory after the job. The worker image
already includes `github-cli`; credentials are neither baked into the image nor mounted from the
desktop host.

Create the `github-default` profile before importing the pack. The profile remains a generic
execution-profile CLI configuration rather than a provider-specific CLI shortcut:

```bash
runinatorctl execution-profiles add \
  --name github-default \
  --description 'GitHub CLI login' \
  --credential-scope github \
  --collection '{"probe":{"argv":["gh","auth","status"]},"sources":[{"type":"directory","path":"~/.config/gh","glob":"*","target":".config/gh"}]}' \
  --exposure '{"home_overlay":true,"environment":{"GH_CONFIG_DIR":"${PROFILE_HOME}/.config/gh"}}'
```

Approve the profile in the desktop agent. `status` shows whether a desktop approved it, has
reported recently, successfully collected credentials, or reported a sanitized error. A ready
publication is required before the Kubernetes worker can download the profile:

```bash
runinatorctl execution-profiles status
runinatorctl execution-profiles status <github-profile-id>
```

Once the profile is published and its status is ready, import and run the job. For a Kubernetes
cluster, keep the web-service port-forward running while using the host CLI:

```bash
bash scripts/port-forward-ws.sh
runinatorctl --api-base-url http://127.0.0.1:8081/ workflows apply packs/github-availability
runinatorctl --api-base-url http://127.0.0.1:8081/ workflows run runinator.tests.github.github_availability
```

You can validate the workflow logic without contacting GitHub:

```bash
runinatorctl workflows test packs/github-availability
```
