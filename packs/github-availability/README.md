# GitHub availability test pack

This pack creates `runinator.tests.github.github_availability`. It runs on a desktop worker with
the `github-default` execution profile and calls GitHub's authenticated `user` endpoint through
the GitHub CLI. A successful response confirms that the desktop agent, approved profile,
materialized credentials, local `gh` executable, and GitHub authentication work together. The
workflow returns `yes` on success; it returns `no` and fails when that integration cannot run.

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
reported recently, successfully collected credentials, or reported a sanitized error:

```bash
runinatorctl execution-profiles status
runinatorctl execution-profiles status <github-profile-id>
```

Once the profile is published and its status is ready, import and run the job:

```bash
runinatorctl workflows apply packs/github-availability
runinatorctl workflows run runinator.tests.github.github_availability
```

You can validate the workflow logic without contacting GitHub:

```bash
runinatorctl workflows test packs/github-availability
```
