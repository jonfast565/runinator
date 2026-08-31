# Desktop agent

Use this guide to run an exclusive desktop worker, configure its connectivity and routing, and operate its replica and recovery behavior.

## Desktop Agent

`runinator-desktop-agent` is a standalone tray application that runs the shared
`runinator-worker` action loop on an operator machine. It publishes the built-in
provider catalog plus the sandboxed local-files provider, and registers as an
exclusive `desktop`-pool replica. It therefore runs only work explicitly pinned
to that replica or targeted to one of its labels; it never picks up unlabeled
general-pool workloads. The Tauri command center is a separate API client and
does not start, stop, embed, or communicate directly with this worker runtime.

```bash
cargo run -p runinator-desktop-agent
```

For an unattended machine, run the same binary without a desktop session:

```bash
cargo run -p runinator-desktop-agent -- \
  --headless \
  --service-url https://runinator.example/ \
  --api-key "$RUNINATOR_API_KEY" \
  --sandbox-root /srv/runinator-agent \
  --labels runner=desktop,zone=home \
  --liveness-file /tmp/runinator-desktop-agent-liveness
```

Command-line values take precedence over environment variables, which take precedence over the
saved GUI configuration. The corresponding environment variables are `RUNINATOR_SERVICE_URL`,
`RUNINATOR_API_KEY`, `RUNINATOR_WORKER_LABELS`, `RUNINATOR_BROKER_MODE`,
`RUNINATOR_MAX_CONCURRENT_ACTIONS`, `RUNINATOR_SHUTDOWN_GRACE_SECONDS`,
`RUNINATOR_RECONNECT_MAX_ATTEMPTS`, and `RUNINATOR_LIVENESS_FILE`.

The tray logo uses its full background as the connection indicator: gray is stopped, blue is
starting or connecting, green is running, amber is retrying, and red needs operator attention. If
the web service or broker becomes unreachable, the agent retries with backoff and also shows an
amber **reconnecting** dot in the window header carrying the attempt number.
After `--reconnect-max-attempts` consecutive failures — 10 by default, roughly seven minutes of
capped backoff — it gives up: the dot turns red **disconnected**, a desktop notification fires, the
replica is marked offline, and the agent stops rather than heartbeating a worker that can never take
an action. Pressing **Start agent** (or restarting the process) tries again. The count is
consecutive, so a connection that stays up clears it; `--reconnect-max-attempts 0` restores retrying
forever. `runinator-worker` takes the same flag but defaults to `0`, since an in-cluster pod's
orchestrator is what decides whether to restart it.

While the agent is coming up — enrolling, registering, or parked waiting for re-enrollment — the
window offers **Cancel startup** in place of **Start agent**. It aborts the attempt wherever it is
waiting, stops anything it already brought up, and returns to the configuration form, so a start
pointed at an unreachable service does not have to be ended by killing the process. "Exit" in the
tray cancels a startup the same way it stops a running agent.

For LAN/local development, `--discover --enroll <token>` (or `RUNINATOR_DISCOVER=true`) listens
for web-service gossip and selects only an announcement whose `cluster_id` matches the identity
bound into that enrollment token. Discovery merely finds an address; the token authorizes the
cluster. Without a bound token, candidates must be chosen explicitly with `--service-url` and are
never auto-enrolled. Gossip is IPv4 UDP broadcast and normally stays within one subnet; it is not
a Kubernetes discovery mechanism. Kubernetes keeps `--disable-gossip` and uses stable service DNS.

Create the one-time token with `runinatorctl agents enroll-token`. Timed enrollment is the default:
the token must be redeemed before `--ttl` elapses, and the machine credential expires at that same
time. Add `--permanent` to issue a machine credential that remains valid until it is explicitly
invalidated. Both modes are available to callers authorized for agent enrollment.

Use `runinatorctl agents machines` to list enrolled machines and
`runinatorctl agents invalidate <machine-id>` to disable one machine, revoke all of its agent
credentials, and kick every replica registered by it. To stop only one current activation while
leaving its machine enrollment usable, run `runinatorctl agents kick <replica-id>`. A kicked replica
id cannot register or heartbeat again; the enrolled machine can start a fresh activation with a new
replica id. The same operations are available in Command Center and its console as `:agents …`.

Web-service announcements include their `http`/`https` scheme, relay path, version, enrollment
availability, and optional SPKI pin. Set `RUNINATOR_CLUSTER_ID` to the same stable UUID on every web
replica when its public enrollment URL differs from the address advertised on the LAN.

`runinatorctl replicas` reads the fleet: `list` (with `--kind`, `--status`, or `--live`), `ids` for
just the identifiers one per line, `show <id>` for one replica and the attributes it heartbeats,
`providers <id>` for what a worker advertises, and `samples <id>` for its recent cpu/memory
telemetry. The same verbs are in both consoles as `:replicas …`. They are read-only on purpose: a
replica row is a report *from* a runtime, so the way to change one is to scale a node group
(`nodes`), direct an agent (`agents`), or stop the process itself.

The service reaps silent replicas after 10 minutes and deletes offline rows after 60 minutes by
default. Set `RUNINATOR_REPLICA_REAP_SECONDS` and `RUNINATOR_REPLICA_DELETE_SECONDS` to tune those
retention windows. Remote agents advertise a separate live/stale window in their heartbeat status,
so normal home-network jitter does not make a connected agent flicker stale.

The control window lets you set the service URL, sandbox folder, optional direct
broker connection, routing labels, and startup behavior. Closing the window
hides it in the tray; use "Exit" from the tray menu to actually quit.

By default the agent relays broker traffic through the web service's
`/ws/broker` endpoint rather than dialing the broker directly, so a machine
behind NAT needs only outbound access to the web service — no inbound ports and
no route to RabbitMQ. The relay URL is derived from the service URL (`https://`
becomes `wss://`), so pointing the agent at a TLS ingress works as-is. Use direct
broker mode only for a machine actually on the broker's network.

The same `--broker-mode direct|relay`, `--service-url`, `--api-key`, and
`--broker-relay-path` connection choice is available to the standalone worker,
waker, engine worker, and archiver. Relay credentials are restricted by their
system role: workers receive their work/control/directive subset, wakers only
receive wakes and settle them, archivers only report lifecycle, and engines can
coordinate all workflow channels. This preserves the normal broker contract
without exposing an unrestricted cluster broker to an outside process.
