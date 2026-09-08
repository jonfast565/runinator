# Claude availability test pack

This test pack creates the `claude` execution profile and a workflow in the
`runinator.tests.claude` namespace: `claude_availability`. The workflow is pinned
to a Kubernetes worker (`runner=kubernetes`), invokes Claude Code with the private
profile using the Haiku model, and publishes `yes` on success. It publishes `no` and fails when
Claude says `no`, or when the Claude invocation cannot start or complete.
The prompt requests the literal `yes`; producing it confirms the authenticated request completed.

The desktop agent only collects and publishes the approved credentials. The Kubernetes worker
downloads the published profile through the worker API, verifies and stages the bundle in a
temporary home directory, runs the prompt, and removes that directory after execution. The worker
image includes the Claude CLI; credentials are never baked into the image or mounted from the host.

To import this pack into Kubernetes, keep a web-service port-forward running in
one terminal and use the host CLI in another:

```bash
bash scripts/port-forward-ws.sh
runinatorctl --api-base-url http://127.0.0.1:8081/ login
runinatorctl --api-base-url http://127.0.0.1:8081/ workflows apply packs/claude-availability
```

The profile probes the installed Claude CLI and collects `~/.claude/CLAUDE.md` plus
`Claude Code-credentials` through the bundled Keychain exporter. It deliberately has no separate
refresh command: requesting rotation reruns those sources and publishes the current desktop login.
If the login itself expires, sign in through Claude Code on the desktop and request rotation again.
A saved local approval remains valid until the collection configuration changes. A macOS Keychain
access prompt is separate from that Runinator approval.

After the desktop agent receives the new profile configuration, approve
`claude` in the agent and let it publish a profile revision. You can
then request the profile's collection dry run and start the workflow:

```bash
runinatorctl --api-base-url http://127.0.0.1:8081/ execution-profiles list
runinatorctl --api-base-url http://127.0.0.1:8081/ execution-profiles test <claude-profile-id>
runinatorctl --api-base-url http://127.0.0.1:8081/ workflows run runinator.tests.claude.claude_availability
```

The workflow has no input parameters. Its final output is `yes` when the check
succeeds; it records `no` before reaching the failed terminal when Claude is not
available. Run the offline pack tests without contacting Claude:

```bash
runinatorctl workflows test packs/claude-availability
```
