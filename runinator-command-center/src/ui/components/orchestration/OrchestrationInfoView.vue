<template>
  <Modal title="Orchestration information" width="min(760px, 100%)" @close="emit('close')">
    <nav class="orchestration-info-nav" aria-label="Information topics">
      <button
        v-for="item in topics"
        :key="item.id"
        type="button"
        class="btn btn-sm"
        :class="topic === item.id ? 'btn-primary' : ''"
        @click="topic = item.id"
      >
        {{ item.label }}
      </button>
    </nav>

    <section v-if="topic === 'behavior'" class="grid gap-3">
      <h3 class="m-0 text-base">Current behavior</h3>
      <ol class="orchestration-info-flow">
        <li v-for="line in summary" :key="line">{{ line }}</li>
      </ol>
      <div class="orchestration-info-diagram" aria-label="Orchestration event flow">
        <span>Event</span><Icon name="chevron-right" /><span>Correlation</span
        ><Icon name="chevron-right" /><span>Lifecycle</span><Icon name="chevron-right" /><span
          >Route or intent</span
        >
      </div>
    </section>

    <section v-else-if="topic === 'terms'" class="grid gap-2">
      <h3 class="m-0 text-base">Terminology</h3>
      <dl class="orchestration-info-terms">
        <div v-for="item in terms" :key="item.term">
          <dt>{{ item.term }}</dt>
          <dd>
            <strong>{{ item.simple }}</strong> · {{ item.description }}
          </dd>
        </div>
      </dl>
    </section>

    <section v-else class="grid gap-2">
      <h3 class="m-0 text-base">Generated policy</h3>
      <pre class="max-h-[32rem] overflow-auto rounded bg-surface-raised p-3 text-xs">{{
        policyText
      }}</pre>
    </section>

    <template #actions>
      <Button @click="emit('close')">Close</Button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import type { CompiledOrchestrationSetup, OrchestrationSetupDraft } from "../../../core/services";
import { setupBehaviorSummary } from "../../../core/services";
import Button from "../shared/Button.vue";
import Icon from "../shared/Icon.vue";
import Modal from "../shared/Modal.vue";

const props = defineProps<{
  draft: OrchestrationSetupDraft;
  compiled: CompiledOrchestrationSetup;
}>();
const emit = defineEmits<{ close: [] }>();
const topic = ref<"behavior" | "terms" | "policy">("behavior");
const topics = [
  { id: "behavior" as const, label: "Behavior" },
  { id: "terms" as const, label: "Terms" },
  { id: "policy" as const, label: "Policy" },
];
const summary = computed(() => setupBehaviorSummary(props.draft));
const policyText = computed(() => JSON.stringify(props.compiled, null, 2));
const terms = [
  {
    term: "Admission route",
    simple: "Incoming event",
    description: "matches an event and chooses what it does in the current lifecycle.",
  },
  {
    term: "Intent",
    simple: "Response",
    description: "controls active work after a matching event arrives.",
  },
  {
    term: "Correlation key",
    simple: "Same-work key",
    description: "groups events that belong to one durable item of work.",
  },
  {
    term: "Epoch",
    simple: "Execution phase",
    description: "is one immutable pipeline execution within an orchestration.",
  },
  {
    term: "Budget",
    simple: "Failure handling",
    description: "limits retries and selects the exhausted outcome.",
  },
  {
    term: "Phase mapping",
    simple: "Saved phase result",
    description: "copies selected workflow output into durable orchestration state.",
  },
];
</script>

<style scoped>
.orchestration-info-nav {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
}
.orchestration-info-flow {
  display: grid;
  gap: 0.55rem;
  margin: 0;
  padding-left: 1.4rem;
  color: var(--color-fg);
  font-size: 0.85rem;
}
.orchestration-info-diagram {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  border: 1px solid var(--color-border);
  border-radius: 0.65rem;
  padding: 0.85rem;
  background: var(--color-bg-subtle);
  font-size: 0.78rem;
  font-weight: 600;
}
.orchestration-info-terms {
  display: grid;
  gap: 0;
  margin: 0;
  border: 1px solid var(--color-border);
  border-radius: 0.65rem;
  overflow: hidden;
}
.orchestration-info-terms div {
  display: grid;
  grid-template-columns: minmax(8rem, 0.35fr) minmax(0, 1fr);
  gap: 0.75rem;
  padding: 0.7rem 0.8rem;
  border-bottom: 1px solid var(--color-border-subtle);
}
.orchestration-info-terms div:last-child {
  border-bottom: 0;
}
.orchestration-info-terms dt {
  font-weight: 700;
}
.orchestration-info-terms dd {
  margin: 0;
  color: var(--color-fg-muted);
}
</style>
