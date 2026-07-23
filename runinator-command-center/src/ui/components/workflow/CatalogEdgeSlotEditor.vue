<template>
  <div class="flex flex-col gap-2">

    <!-- branch taxonomy with multiple entries: list of {when, target} pairs. -->
    <template v-if="edgeSlot.taxonomy === 'branch' && edgeSlot.multiple">
      <div
        v-for="(branch, index) in branchList"
        :key="index"
        class="branch-row"
      >
        <div class="form-field">
          <span class="form-field-label">When</span>
          <ExpressionJsonEditor
            v-model="branch.when_json"
            :context="expressionContext ?? undefined"
            :title="`${edgeSlot.label} ${String(index + 1)}`"
            @update:model-value="(v) => { branch.when_json = v; emitBranches(); }"
          />
        </div>
        <label>
          Target
          <select v-model="branch.target" @change="emitBranches">
            <option value="">(none)</option>
            <option v-for="nodeId in nodeOptions" :key="nodeId" :value="nodeId">{{ nodeId }}</option>
          </select>
        </label>
        <button type="button" @click="removeBranch(index)">Remove</button>
      </div>
      <button type="button" @click="addBranch">Add Branch</button>
    </template>

    <!-- control taxonomy, multiple=true, key=cases: switch-style case list. -->
    <template v-else-if="edgeSlot.taxonomy === 'control' && edgeSlot.multiple && edgeSlot.key === 'cases'">
      <div
        v-for="(switchCase, index) in switchCaseList"
        :key="index"
        class="branch-row"
      >
        <label>
          Match
          <select
            v-if="matchKindsLoaded"
            v-model="switchCase.match_kind"
            @change="emitSwitchCases"
          >
            <option v-for="opt in matchKindOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </select>
          <span v-else class="hint">Loading match kinds…</span>
        </label>
        <div class="form-field">
          <span class="form-field-label">Value</span>
          <ExpressionJsonEditor
            v-model="switchCase.match_json"
            :context="expressionContext ?? undefined"
            :title="`Case ${String(index + 1)} match`"
            @update:model-value="(v) => { switchCase.match_json = v; emitSwitchCases(); }"
          />
        </div>
        <label>
          Target
          <select v-model="switchCase.target" @change="emitSwitchCases">
            <option value="">(none)</option>
            <option v-for="nodeId in nodeOptions" :key="nodeId" :value="nodeId">{{ nodeId }}</option>
          </select>
        </label>
        <button type="button" @click="removeSwitchCase(index)">Remove</button>
      </div>
      <button type="button" @click="addSwitchCase">Add Case</button>
    </template>

    <!-- control taxonomy, multiple=true, key=buckets: weighted percentage buckets. -->
    <template v-else-if="edgeSlot.taxonomy === 'control' && edgeSlot.multiple && edgeSlot.key === 'buckets'">
      <div
        v-for="(bucket, index) in bucketList"
        :key="index"
        class="branch-row"
      >
        <label>
          Weight
          <input v-model.number="bucket.weight" type="number" min="1" @input="emitBuckets" />
        </label>
        <span class="form-field-label">{{ bucketShare(index) }}</span>
        <label>
          Target
          <select v-model="bucket.target" @change="emitBuckets">
            <option value="">(none)</option>
            <option v-for="nodeId in nodeOptions" :key="nodeId" :value="nodeId">{{ nodeId }}</option>
          </select>
        </label>
        <button type="button" @click="removeBucket(index)">Remove</button>
      </div>
      <button type="button" @click="addBucket">Add Bucket</button>
    </template>

    <!-- control taxonomy, multiple=true, generic: list of node refs (parallel, join wait_for, race). -->
    <template v-else-if="edgeSlot.taxonomy === 'control' && edgeSlot.multiple">
      <div
        v-for="(_, index) in nodeRefList"
        :key="index"
        class="branch-row"
      >
        <label>
          Node
          <select v-model="nodeRefList[index]" @change="emitNodeRefs">
            <option value="">(none)</option>
            <option v-for="nodeId in nodeOptions" :key="nodeId" :value="nodeId">{{ nodeId }}</option>
          </select>
        </label>
        <button type="button" @click="removeNodeRef(index)">Remove</button>
      </div>
      <button type="button" @click="addNodeRef">Add Node</button>
    </template>

    <!-- control taxonomy, single: one node select stored as a node-ref object. -->
    <template v-else>
      <select :value="singleNodeValue" @change="emitSingleNode(($event.target as HTMLSelectElement).value)">
        <option value="">(none)</option>
        <option v-for="nodeId in nodeOptions" :key="nodeId" :value="nodeId">{{ nodeId }}</option>
      </select>
    </template>

  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { pretty } from "../../../core/utils/format";
import { parseRequiredJson } from "../../../core/utils/json";
import { nodeRef as makeNodeRef, nodeRefId } from "../../../core/workflow/index";
import type { NodeEdgeSlot } from "../../../core/domain/models";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";

const props = withDefaults(defineProps<{
  edgeSlot: NodeEdgeSlot;
  modelValue: unknown;
  nodeOptions?: string[];
  expressionContext?: object | null;
}>(), {
  nodeOptions: () => [],
  expressionContext: null,
});

const catalogMetadata = useCatalogMetadataStore();

// match_kind options driven from catalog only.
const matchKindOptions = computed(() => catalogMetadata.enumOptions("match_kind"));
const matchKindsLoaded = computed(() => matchKindOptions.value.length > 0);

const emit = defineEmits<(e: "update:modelValue", value: unknown) => void>();

// --- branch list (condition branches) ---

interface BranchDraft {
  when_json: string;
  target: string;
}

const branchList = ref<BranchDraft[]>(buildBranchList(props.modelValue));

watch(() => props.modelValue, (next) => {
  branchList.value = buildBranchList(next);
});

function buildBranchList(value: unknown): BranchDraft[] {
  if (!Array.isArray(value)) {return [];}
  return value.map((item) => {
    const rec = toRec(item);
    return {
      when_json: pretty(rec.when ?? {}),
      target: nodeRefId(rec.target) ?? "",
    };
  });
}

function emitBranches() {
  emit("update:modelValue", branchList.value.map((b) => {
    const when = parseRequiredJson(b.when_json) ?? {};
    const result: Record<string, unknown> = { when };
    if (b.target) {result.target = makeNodeRef(b.target);}
    return result;
  }));
}

function addBranch() {
  branchList.value.push({ when_json: pretty(true), target: "" });
  emitBranches();
}

function removeBranch(index: number) {
  branchList.value.splice(index, 1);
  emitBranches();
}

// --- switch case list ---

interface SwitchCaseDraft {
  match_kind: string;
  match_json: string;
  target: string;
}

const switchCaseList = ref<SwitchCaseDraft[]>(buildSwitchCaseList(props.modelValue));

watch(() => props.modelValue, (next) => {
  switchCaseList.value = buildSwitchCaseList(next);
});

function buildSwitchCaseList(value: unknown): SwitchCaseDraft[] {
  if (!Array.isArray(value)) {return [];}
  return value.map((item) => {
    const rec = toRec(item);
    const target = nodeRefId(rec.target) ?? "";

    if (rec.when !== undefined) {
      return { match_kind: "when", match_json: pretty(rec.when), target };
    }

    if (rec.not_equals !== undefined) {
      return { match_kind: "not_equals", match_json: pretty(rec.not_equals), target };
    }

    if (rec.exists !== undefined) {
      return { match_kind: "exists", match_json: pretty(Boolean(rec.exists)), target };
    }

    return { match_kind: "equals", match_json: pretty(rec.equals ?? ""), target };
  });
}

function emitSwitchCases() {
  emit("update:modelValue", switchCaseList.value.map((c) => {
    const match = parseRequiredJson(c.match_json) ?? "";
    const result: Record<string, unknown> = { target: makeNodeRef(c.target) };

    if (c.match_kind === "when") {result.when = match;}
    else if (c.match_kind === "exists") {result.exists = Boolean(match);}
    else {result[c.match_kind] = match;}

    return result;
  }));
}

function addSwitchCase() {
  switchCaseList.value.push({ match_kind: "equals", match_json: pretty(true), target: "" });
  emitSwitchCases();
}

function removeSwitchCase(index: number) {
  switchCaseList.value.splice(index, 1);
  emitSwitchCases();
}

// --- percentage bucket list ---

interface BucketDraft {
  weight: number;
  target: string;
}

const bucketList = ref<BucketDraft[]>(buildBucketList(props.modelValue));

watch(() => props.modelValue, (next) => {
  bucketList.value = buildBucketList(next);
});

function buildBucketList(value: unknown): BucketDraft[] {
  if (!Array.isArray(value)) {return [];}
  return value.map((item) => {
    const rec = toRec(item);
    return {
      weight: typeof rec.weight === "number" ? rec.weight : 0,
      target: nodeRefId(rec.target) ?? "",
    };
  });
}

function emitBuckets() {
  emit("update:modelValue", bucketList.value.map((b) => ({
    weight: Math.max(1, Math.trunc(b.weight)),
    target: makeNodeRef(b.target),
  })));
}

function bucketShare(index: number): string {
  const total = bucketList.value.reduce((sum, b) => sum + (b.weight || 0), 0);
  const weight = bucketList.value[index]?.weight || 0;
  if (total <= 0) {return "—";}
  return `${String(Math.round((weight / total) * 100))}%`;
}

function addBucket() {
  bucketList.value.push({ weight: 50, target: "" });
  emitBuckets();
}

function removeBucket(index: number) {
  bucketList.value.splice(index, 1);
  emitBuckets();
}

// --- generic node ref list (parallel branches, join wait_for) ---

const nodeRefList = ref<string[]>(buildNodeRefList(props.modelValue));

watch(() => props.modelValue, (next) => {
  nodeRefList.value = buildNodeRefList(next);
});

function buildNodeRefList(value: unknown): string[] {
  if (!Array.isArray(value)) {return [];}
  return value.map((item) => nodeRefId(item) ?? "").filter(Boolean);
}

function emitNodeRefs() {
  emit("update:modelValue", nodeRefList.value.filter(Boolean).map(makeNodeRef));
}

function addNodeRef() {
  nodeRefList.value.push("");
  emitNodeRefs();
}

function removeNodeRef(index: number) {
  nodeRefList.value.splice(index, 1);
  emitNodeRefs();
}

// --- single node ref ---

const singleNodeValue = computed(() => nodeRefId(props.modelValue) ?? "");

function emitSingleNode(nodeId: string) {
  emit("update:modelValue", nodeId ? makeNodeRef(nodeId) : undefined);
}

// --- utility ---

function toRec(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
</script>
