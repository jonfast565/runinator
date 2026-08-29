<template>
  <section class="detail-section header-section">
    <div class="flex items-center gap-1">
      <h3>Watch guards</h3>
      <HelpBubble
        text="Watch guards are re-evaluated on every drive, including while the run is parked. The first matching guard wins and fires at most once per run."
        label="About watch guards"
      />
    </div>

    <p v-if="watches.length === 0" class="hint">No watch guards declared.</p>

    <div v-for="(entry, index) in watches" :key="index" class="header-row">
      <ExpressionJsonEditor
        :model-value="conditionJson(entry.condition)"
        title="Watch condition"
        :context="expressionContext"
        @update:model-value="(value: string) => setCondition(index, value)"
      />
      <div class="header-row-controls">
        <label
          >Jump to
          <select
            :value="entry.handler"
            @change="workflows.setHeaderWatch(index, { handler: selectValue($event) })"
          >
            <option v-for="id in targets" :key="id" :value="id">{{ id }}</option>
          </select>
        </label>
        <button type="button" class="btn btn-sm" @click="workflows.removeHeaderWatch(index)">
          Remove
        </button>
      </div>
    </div>

    <div class="header-actions">
      <button type="button" class="btn btn-sm" @click="workflows.addHeaderWatch">Add guard</button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useProvidersStore } from "../../adapters/pinia/providers";
import { pretty } from "../../../core/utils/format";
import { parseRequiredObject } from "../../../core/utils/json";
import { isJsonRecord as isRecord, jsonRecordArray as asArray } from "../../../core/domain/json";
import { workflowInputType } from "../../../core/domain/models";
import { buildSampleContext } from "../../../core/utils/workflow-references";
import { displayValue } from "../../../core/utils/values";
import type { JsonRecord, JsonValue } from "../../../core/domain/json";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";
import HelpBubble from "../shared/HelpBubble.vue";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();

const watches = computed(() => workflows.headerDraft.watches);

// `end` and `fail` are always legal targets: rexrap spells them `done` and `fail`.
const targets = computed(() => {
  void workflows.workflowLayoutVersion;
  const ids = asArray(workflows.workflowDraft.definition.nodes)
    .filter((node): node is JsonRecord => isRecord(node))
    .map((node) => displayValue(node.id))
    .filter(Boolean);

  return [...new Set([...ids, "end", "fail"])];
});

const expressionContext = computed(() => ({
  workflowInputType: workflowInputType(workflows.workflowDraft),
  nodes: asArray(workflows.workflowDraft.definition.nodes).filter((node): node is JsonRecord =>
    isRecord(node),
  ),
  currentNodeId: "",
  providers: providersStore.providers,
  sampleContext: buildSampleContext(workflows.workflowRunDetail),
}));

function conditionJson(condition: JsonValue): string {
  return pretty(condition ?? {});
}

function setCondition(index: number, value: string) {
  const parsed = parseRequiredObject(value);

  // an unparseable draft is the editor's own inline error; do not write it into the definition,
  // where it would break decompile and take the rexrap pane down with it.
  if (!parsed) {
    return;
  }

  workflows.setHeaderWatch(index, { condition: parsed as JsonValue });
}

function selectValue(event: Event): string {
  return (event.target as HTMLSelectElement).value;
}
</script>
