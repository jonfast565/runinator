# Adapter and orchestration debugging

Open **Ingress Control → Adapter Events**, choose an adapter, and select **Hold for review** to stop newly captured normalized deliveries before pipeline routing and admission. The same inspector is available under **Orchestrations → Adapters → Messages**. Expand a delivery to see its received identity, normalized payload, canonical routing identity, route/intent preview, admission result, adapter revision, and poll-attempt correlation. Approve, retry, or drop individual deliveries. Requests that failed verification or normalization have no executable event and cannot be approved.

The adapter gate is durable. Returning it to **Observe and admit** admits new deliveries; already held or failed deliveries still require an explicit decision. Pipeline/workflow review is a separate gate under **External Events**. A delivery held there points to its review record. Approval is queued durably, with a five-minute application lease and recovery after engine interruption. Failed approvals can be retried. Adapter identity/revision survives the review queue, and start-event identity is reused when recovering a run creation.

Under **Orchestrations → Instances**, **Orchestration transitions** pauses the selected pipeline's reducers before their next event or due intent. **Step once** permits one transition across that pipeline's bindings. Resume restores normal reduction. A transition already in progress may finish; running provider work and explicit operator commands remain independent of this reducer gate. Broker ingress holds still apply only to their specific broker messages.

Polling with an execution profile uses the ordinary worker effect path. **Test** returns a durable job immediately and displays its result when the worker responds. Test jobs normalize and preview events without admitting them or changing a live checkpoint. Alias resolution and subject-revision/provenance enrichment are shared with live delivery preparation. Worker-backed poll/test attempts retain their publication state, deadline, outcome, and error, and adapter broker messages use adapter/attempt identity instead of masquerading as workflow-run messages.

Normalized poll events enter the delivery journal before a checkpoint can advance. Routing failures and full downstream review queues therefore retain the delivery for retry. A capture failure retains the old checkpoint. Polling and webhook normalization both extract external-operation provenance for self-origin suppression. GitHub initialization establishes a current boundary without replaying history; a later scan exceeding the page or commit budget fails visibly without advancing its checkpoint. It does not silently truncate the event batch.

Completed delivery history and terminal poll/test attempts are retained for seven days. Held and failed deliveries remain available. The inspector shows the most recent 500 deliveries and 100 attempts; the journal rejects new captures at 10,000 unresolved deliveries per adapter. Poll/test publication has a recoverable 30-second lease and a five-minute attempt deadline. Late terminal updates cannot replace an already terminal attempt. Diagnostic responses redact common secret fields without changing the durable event used for execution.

## API

All endpoints require authorization against the stored adapter or pipeline resource. Viewing uses View permission; operator decisions use Run permission. Webhook verification remains the public provider-facing boundary.

| Method and path | Behavior |
| --- | --- |
| `GET /orchestrations/adapters/{id}/deliveries` | Recent normalized deliveries and routing/admission outcomes |
| `GET /orchestrations/adapters/{id}/attempts` | Poll attempts and worker-backed test results |
| `GET/PUT /orchestrations/adapters/{id}/inspection` | Read/change `{ "mode": "disabled" \| "review" \| "paused" }` |
| `POST /orchestrations/adapters/{id}/deliveries/{delivery_id}/{decision}` | `approve`, `retry`, or `drop` |
| `GET/PUT /pipelines/{id}/orchestration-debug` | Read/change `{ "paused": true, "steps": 0 }`; one step uses `steps: 1` |
| `POST /orchestrations/adapters/{id}/test` | Profile polling returns `202 { "job_id": "…", "state": "queued" }`; inline previews return 200 |
| `GET /broker_messages?adapter_id={id}` | Adapter-scoped broker trace |

## Focused verification

One regression case covers each of the ten audit items: two adapter-host cases for scan bounds and provenance, three engine cases for durable capture, profile test isolation and alias parity, and five shared SQL cases for reviewed origin, approval recovery, journal correlation, reducer permits and poll publication/history. The SQL cases run unchanged against SQLite, Postgres and MariaDB. Existing compilation, lint, route/schema and build checks cover integration wiring.
