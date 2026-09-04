# Claude availability test pack

This test pack creates the `claude` execution profile and a workflow in the
`runinator.tests.claude` namespace: `claude_availability`. The workflow is pinned
to the desktop worker (`runner=desktop`), invokes Claude Code with the private
profile, and publishes `yes` on success. It publishes `no` and fails when Claude
says `no`, or when the Claude invocation cannot start or complete.

The Kubernetes pack-import job includes this directory with the other bundled
packs. After deploying an image that contains this pack, re-run only that job:

```bash
cargo run -p xtask -- k8s import-packs
```

For a local or non-Kubernetes target, apply this individual pack directly:

```bash
runinatorctl workflows apply packs/claude-availability
```

After the desktop agent receives the new profile configuration, approve
`claude` in the agent and let it publish a profile revision. You can
then request the profile's collection dry run and start the workflow:

```bash
runinatorctl execution-profiles list
runinatorctl execution-profiles test <claude-profile-id>
runinatorctl workflows run runinator.tests.claude.claude_availability
```

The workflow has no input parameters. Its final output is `yes` when the check
succeeds; it records `no` before reaching the failed terminal when Claude is not
available. Run the offline pack tests without contacting Claude:

```bash
runinatorctl workflows test packs/claude-availability
```
