<template>
  <section class="detail-section header-section">
    <div class="flex items-center gap-1">
      <h3>Correlation key</h3>
      <HelpBubble label="About correlation keys">
        The value this workflow's runs are awaitable by. It is resolved as the run progresses and
        stamped write-once, so another workflow's <code>await workflow … key</code> can match it.
      </HelpBubble>
    </div>

    <p v-if="workflows.headerDraft.correlation === null" class="hint">
      No correlation key: runs of this workflow cannot be awaited by key.
    </p>

    <ExpressionJsonEditor
      :model-value="correlationJson"
      title="Correlation expression"
      :context="expressionContext"
      @update:model-value="setCorrelation"
    />

    <div class="header-actions">
      <button
        v-if="workflows.headerDraft.correlation !== null"
        type="button"
        class="btn btn-sm"
        @click="workflows.setHeaderCorrelation(null)"
      >
        Clear
      </button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useProvidersStore } from "../../adapters/pinia/providers";
import { pretty } from "../../../core/utils/format";
import { isJsonRecord as isRecord, jsonRecordArray as asArray } from "../../../core/domain/json";
import { workflowInputType } from "../../../core/domain/models";
import { buildSampleContext } from "../../../core/utils/workflow-references";
import type { JsonRecord, JsonValue } from "../../../core/domain/json";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";
import HelpBubble from "../shared/HelpBubble.vue";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();

const correlationJson = computed(() => pretty(workflows.headerDraft.correlation ?? {}));

const expressionContext = computed(() => ({
  workflowInputType: workflowInputType(workflows.workflowDraft),
  nodes: asArray(workflows.workflowDraft.definition.nodes).filter((node): node is JsonRecord =>
    isRecord(node),
  ),
  currentNodeId: "",
  providers: providersStore.providers,
  sampleContext: buildSampleContext(workflows.workflowRunDetail),
}));

function setCorrelation(value: string) {
  try {
    workflows.setHeaderCorrelation(JSON.parse(value) as JsonValue);
  } catch {
    // the editor shows its own parse error; leave the last good expression in place.
  }
}
</script>
