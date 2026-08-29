<template>
  <section class="form-section revisions-section">
    <div class="section-toolbar">
      <div class="flex items-center gap-1">
        <h3>Revision history</h3>
        <HelpBubble
          text="Each save records a revision. Restoring re-validates the old definition and saves it as a new revision, so nothing is overwritten and the rollback remains in history."
          label="About workflow revisions"
        />
      </div>
      <div class="section-actions">
        <button type="button" :disabled="!workflowId || loading" @click="refresh">Refresh</button>
      </div>
    </div>

    <p v-if="!workflowId" class="hint">Save the workflow before it has a history.</p>

    <EmptyState
      v-else-if="loading && revisions.length === 0"
      title="Loading revision history"
      loading
      compact
    />

    <EmptyState
      v-else-if="revisions.length === 0"
      title="No revisions yet"
      description="Each save records the definition here, so a change can be compared and rolled back."
      compact
    />

    <template v-else>
      <div class="revision-table-wrap">
        <DataTable bare compact>
          <thead>
            <tr>
              <th class="col-pick" :title="compareHint">A</th>
              <th class="col-pick" :title="compareHint">B</th>
              <th>Rev</th>
              <th>Version</th>
              <th>Source</th>
              <th>Author</th>
              <th>When</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="revision in revisions"
              :key="revision.id"
              :class="{ current: revision.revision === headRevision }"
            >
              <td class="col-pick">
                <input
                  type="radio"
                  name="revision-compare-a"
                  :value="revision.revision"
                  :checked="compareA === revision.revision"
                  :aria-label="`Compare from revision ${revision.revision}`"
                  @change="compareA = revision.revision"
                />
              </td>
              <td class="col-pick">
                <input
                  type="radio"
                  name="revision-compare-b"
                  :value="revision.revision"
                  :checked="compareB === revision.revision"
                  :aria-label="`Compare to revision ${revision.revision}`"
                  @change="compareB = revision.revision"
                />
              </td>
              <td>
                {{ revision.revision }}
                <span v-if="revision.revision === headRevision" class="current-tag">current</span>
              </td>
              <td>{{ revision.version }}</td>
              <td>
                <span class="source-tag" :class="`source-${revision.source}`">{{
                  revision.source
                }}</span>
              </td>
              <td :title="revisionAuthorLabel(revision)">{{ revision.actor_kind }}</td>
              <td>{{ formatWhen(revision.created_at) }}</td>
              <td class="row-actions">
                <button
                  type="button"
                  :disabled="restoring !== null || revision.revision === headRevision"
                  :title="
                    revision.revision === headRevision
                      ? 'Already the current definition'
                      : `Restore revision ${revision.revision}`
                  "
                  @click="restore(revision)"
                >
                  {{ restoring === revision.revision ? "Restoring…" : "Restore" }}
                </button>
              </td>
            </tr>
          </tbody>
        </DataTable>
      </div>

      <JsonDiff
        v-if="diffPair"
        :before="diffPair.before.definition"
        :after="diffPair.after.definition"
        :title="`Diff (revision ${diffPair.before.revision} → ${diffPair.after.revision})`"
        open
      />
      <p v-else-if="compareA === compareB" class="hint">
        Pick two different revisions to compare them.
      </p>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { WorkflowDefinition, WorkflowRevision } from "../../../core/domain/models";
import { revisionAuthorLabel } from "../../../core/domain/models";
import { workflowRevisionsService } from "../../../core/services";
import { useAppStore } from "../../adapters/pinia/app";
import EmptyState from "../shared/EmptyState.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import JsonDiff from "./JsonDiff.vue";

const props = defineProps<{ workflowId: string | null }>();
// carries the restored definition so the editor can reset its draft to it. without that the open
// draft still holds the definition that was just rolled back, and the next save would undo the
// rollback.
const emit = defineEmits<{ restored: [workflow: WorkflowDefinition] }>();

const app = useAppStore();

const revisions = ref<WorkflowRevision[]>([]);
const loading = ref(false);
const restoring = ref<number | null>(null);
const compareA = ref<number | null>(null);
const compareB = ref<number | null>(null);

const compareHint = "Pick two revisions to diff";

// the newest revision is the one matching the stored definition.
const headRevision = computed(() => (revisions.value.length ? revisions.value[0].revision : null));

/** ordered oldest → newest so the diff reads as "what changed", not "what was undone". */
const diffPair = computed(() => {
  if (compareA.value === null || compareB.value === null || compareA.value === compareB.value) {
    return null;
  }

  const a = revisions.value.find((r) => r.revision === compareA.value);
  const b = revisions.value.find((r) => r.revision === compareB.value);

  if (!a || !b) {
    return null;
  }

  return a.revision < b.revision ? { before: a, after: b } : { before: b, after: a };
});

function formatWhen(value: string | null | undefined): string {
  if (!value) {
    return "-";
  }

  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? value : new Date(parsed).toLocaleString();
}

async function refresh() {
  if (!props.workflowId) {
    revisions.value = [];
    return;
  }

  loading.value = true;

  try {
    revisions.value = await workflowRevisionsService.list(props.workflowId);
    // default the comparison to the most recent change, which is what someone opening this
    // panel after a bad save is looking for.
    compareB.value = revisions.value[0]?.revision ?? null;
    compareA.value = revisions.value[1]?.revision ?? null;
  } catch {
    // runOperation already surfaced the failure; keep whatever was on screen.
  } finally {
    loading.value = false;
  }
}

async function restore(revision: WorkflowRevision) {
  if (!props.workflowId) {
    return;
  }

  restoring.value = revision.revision;

  try {
    const restored = await workflowRevisionsService.restore(props.workflowId, revision.revision);
    app.setStatus(`Restored revision ${String(revision.revision)}`);
    emit("restored", restored);
    await refresh();
  } catch (error) {
    app.setError(String(error));
  } finally {
    restoring.value = null;
  }
}

watch(() => props.workflowId, refresh, { immediate: true });
</script>

<style scoped>
.revision-table-wrap {
  overflow-x: auto;
}

.col-pick {
  width: 2rem;
  text-align: center;
}

tr.current td {
  font-weight: 600;
}

.current-tag,
.source-tag {
  font-size: 0.75em;
  padding: 0.05em 0.4em;
  border-radius: 0.25em;
  border: 1px solid currentColor;
  opacity: 0.75;
}

.current-tag {
  margin-left: 0.4em;
}

/* a pack apply is the change most likely to need undoing, so it reads differently at a glance. */
.source-pack,
.source-rollback {
  opacity: 1;
}
</style>
