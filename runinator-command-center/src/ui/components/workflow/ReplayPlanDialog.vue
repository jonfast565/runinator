<template>
  <dialog ref="dialog" aria-labelledby="replay-plan-title" @cancel.prevent="finish(false)">
    <form @submit.prevent="finish(true)">
      <h2 id="replay-plan-title">Replay safety · {{ plan.verdict }}</h2>
      <p>
        {{ plan.workflow_snapshot?.name ?? "Unavailable snapshot" }} ·
        {{ plan.from_step_id ? `Restart at ${plan.from_step_id}` : "Restart from the beginning" }}
      </p>
      <p class="hint">Uses the original frozen workflow. No work has been dispatched.</p>
      <ul v-if="plan.reasons.length">
        <li v-for="reason in plan.reasons" :key="reason">{{ reason }}</li>
      </ul>
      <h3>Seeded ancestor receipts ({{ plan.seeded_receipts.length }})</h3>
      <p v-if="!plan.seeded_receipts.length" class="hint">
        No prior action results will be copied.
      </p>
      <ul v-else>
        <li v-for="receipt in plan.seeded_receipts" :key="receipt.effect_id">
          <strong>{{ receipt.node_id }}</strong> · attempt {{ receipt.attempt }}<br /><code>{{
            receipt.effect_id
          }}</code>
        </li>
      </ul>
      <h3>Actions that may run ({{ plan.actions.length }})</h3>
      <article v-for="(action, index) in plan.actions" :key="`${action.node_id}-${index}`">
        <strong>{{ action.node_id }} · {{ action.provider }}.{{ action.function }}</strong>
        <p>{{ action.reason }}</p>
        <details>
          <summary>Idempotency evidence</summary>
          <pre>
Declared: {{ JSON.stringify(action.declared_idempotency_key, null, 2) }}
Previously resolved: {{ JSON.stringify(action.previous_resolved_idempotency_keys, null, 2) }}</pre>
        </details>
      </article>
      <details>
        <summary>Frozen definition</summary>
        <pre>{{ JSON.stringify(plan.workflow_snapshot, null, 2) }}</pre>
      </details>
      <label v-if="plan.verdict === 'review'" class="acknowledgement"
        ><input v-model="acknowledged" type="checkbox" />I understand that this replay may duplicate
        external side effects.</label
      >
      <footer>
        <button type="button" class="btn" @click="finish(false)">Cancel</button
        ><button
          type="submit"
          class="btn btn-primary"
          :disabled="plan.verdict === 'blocked' || (plan.verdict === 'review' && !acknowledged)"
        >
          Start replay
        </button>
      </footer>
    </form>
  </dialog>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import type { ReplayPlan } from "../../../core/domain/models/workflow/replay";
defineProps<{ plan: ReplayPlan }>();
const emit = defineEmits<{ complete: [accepted: boolean] }>();
const dialog = ref<HTMLDialogElement>();
const acknowledged = ref(false);
onMounted(() => dialog.value?.showModal());

function finish(accepted: boolean) {
  dialog.value?.close();
  emit("complete", accepted);
}
</script>

<style scoped>
dialog {
  margin: auto;
  overflow-wrap: anywhere;
  color: var(--text);
  background: var(--surface, #171b24);
  border: 1px solid var(--border-subtle, #4b5563);
  border-radius: 12px;
  padding: 24px;
  width: min(760px, calc(100vw - 32px));
  max-height: 85vh;
  overflow: auto;
}
dialog::backdrop {
  background: #0009;
}
h2 {
  font-size: 1.25rem;
}
h3 {
  margin-top: 20px;
}
p,
li {
  margin: 8px 0;
}
ul {
  padding-left: 20px;
}
article {
  padding: 12px 0;
  border-bottom: 1px solid var(--border-subtle, #4b5563);
}
pre,
code {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-size: 12px;
}
details {
  margin-top: 12px;
}
.acknowledgement {
  display: flex;
  gap: 10px;
  margin: 24px 0;
}
footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 24px;
}
</style>
