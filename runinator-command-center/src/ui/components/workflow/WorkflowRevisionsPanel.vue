<template>
  <section class="form-section revisions-section" :class="{ 'is-open': expanded }">
    <div class="revisions-heading">
      <button
        type="button"
        class="revisions-disclosure"
        :aria-expanded="expanded"
        aria-controls="workflow-revision-history"
        @click="expanded = !expanded"
      >
        <span class="revisions-icon"><Icon name="restart" :size="16" /></span>
        <span class="revisions-heading-copy">
          <span class="revisions-title-line">
            <strong>Revision history</strong>
            <span v-if="revisions.length" class="revision-count">{{ revisions.length }}</span>
          </span>
          <small>{{ revisionSummary }}</small>
        </span>
        <Icon class="revisions-chevron" name="chevron-right" :size="15" />
      </button>
      <div class="revisions-heading-actions">
        <HelpBubble
          text="Each save records a revision. Restoring re-validates the old definition and saves it as a new revision, so nothing is overwritten and the rollback remains in history."
          label="About workflow revisions"
        />
        <button
          type="button"
          class="btn btn-icon"
          title="Refresh revision history"
          aria-label="Refresh revision history"
          :disabled="!workflowId || loading"
          @click="refresh"
        >
          <Icon name="refresh" :size="13" />
        </button>
      </div>
    </div>

    <div v-if="expanded" id="workflow-revision-history" class="revisions-content">
      <p v-if="!workflowId" class="hint revision-callout">
        Save the workflow to start its revision history.
      </p>

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
        <div v-if="revisions.length > 1" class="revision-compare-panel">
          <div class="revision-compare-heading">
            <span class="revision-compare-icon"><Icon name="branch" :size="15" /></span>
            <div>
              <strong>Compare changes</strong>
              <small>Choose an earlier and later revision.</small>
            </div>
          </div>
          <div class="revision-compare-fields">
            <label>
              <span>From</span>
              <select v-model.number="compareA">
                <option v-for="revision in revisions" :key="revision.id" :value="revision.revision">
                  Revision {{ revision.revision }} · {{ formatWhen(revision.created_at) }}
                </option>
              </select>
            </label>
            <span class="revision-compare-arrow"><Icon name="arrow-down" :size="14" /></span>
            <label>
              <span>To</span>
              <select v-model.number="compareB">
                <option v-for="revision in revisions" :key="revision.id" :value="revision.revision">
                  Revision {{ revision.revision }} · {{ formatWhen(revision.created_at) }}
                </option>
              </select>
            </label>
          </div>
          <p v-if="compareA === compareB" class="revision-compare-hint">
            Choose two different revisions to see what changed.
          </p>
        </div>

        <JsonDiff
          v-if="diffPair"
          :before="diffPair.before.definition"
          :after="diffPair.after.definition"
          :title="`View changes from revision ${diffPair.before.revision} to ${diffPair.after.revision}`"
        />

        <div class="revision-list">
          <article
            v-for="revision in revisions"
            :key="revision.id"
            class="revision-card"
            :class="{ 'is-current': revision.revision === headRevision }"
          >
            <div class="revision-number">
              <span>Revision</span>
              <strong>{{ revision.revision }}</strong>
            </div>
            <div class="revision-card-content">
              <div class="revision-card-title">
                <strong>Version {{ revision.version }}</strong>
                <span v-if="revision.revision === headRevision" class="current-tag">Current</span>
                <span class="source-tag" :class="`source-${revision.source}`">
                  {{ sourceLabel(revision.source) }}
                </span>
              </div>
              <p v-if="revision.note" class="revision-note">{{ revision.note }}</p>
              <div class="revision-meta">
                <span><Icon name="clock" :size="12" />{{ formatWhen(revision.created_at) }}</span>
                <span :title="revisionAuthorLabel(revision)">
                  <Icon name="user" :size="12" />{{ actorLabel(revision.actor_kind) }}
                </span>
              </div>
            </div>
            <div class="revision-card-action">
              <span v-if="revision.revision === headRevision" class="revision-current-label">
                Current definition
              </span>
              <button
                v-else
                type="button"
                class="btn"
                :disabled="restoring !== null"
                @click="restore(revision)"
              >
                <Icon name="restart" :size="13" />
                {{ restoring === revision.revision ? "Restoring…" : "Restore" }}
              </button>
            </div>
          </article>
        </div>
      </template>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type {
  RevisionSource,
  WorkflowDefinition,
  WorkflowRevision,
} from "../../../core/domain/models";
import { revisionAuthorLabel } from "../../../core/domain/models";
import { workflowRevisionsService } from "../../../core/services";
import { formatDate } from "../../../core/utils/format";
import { useAppStore } from "../../adapters/pinia/app";
import EmptyState from "../shared/EmptyState.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import Icon from "../shared/Icon.vue";
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
const expanded = ref(false);

// the newest revision is the one matching the stored definition.
const headRevision = computed(() => (revisions.value.length ? revisions.value[0].revision : null));
const revisionSummary = computed(() => {
  if (!props.workflowId) {
    return "Available after the first save";
  }

  if (loading.value && revisions.value.length === 0) {
    return "Loading saved versions…";
  }

  if (revisions.value.length === 0) {
    return "No saved versions yet";
  }

  return `${String(revisions.value.length)} saved ${revisions.value.length === 1 ? "version" : "versions"} · current revision ${String(headRevision.value)}`;
});

/** ordered oldest → newest so the diff reads as "what changed", not "what was undone". */
const diffPair = computed(() => {
  if (compareA.value === null || compareB.value === null || compareA.value === compareB.value) {
    return null;
  }

  const a = revisions.value.find((revision) => revision.revision === compareA.value);
  const b = revisions.value.find((revision) => revision.revision === compareB.value);

  if (!a || !b) {
    return null;
  }

  return a.revision < b.revision ? { before: a, after: b } : { before: b, after: a };
});

function formatWhen(value: string | null | undefined): string {
  return value ? formatDate(value) : "Date unavailable";
}

function sourceLabel(source: RevisionSource): string {
  const labels: Record<RevisionSource, string> = {
    ui: "Editor",
    pack: "Pack import",
    api: "API",
    duplicate: "Duplicate",
    rollback: "Restore",
  };

  return labels[source];
}

function actorLabel(kind: string): string {
  return kind.replaceAll("_", " ").replace(/\b\w/g, (character) => character.toUpperCase());
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
.revisions-section {
  display: grid;
  gap: 0;
  padding: 0;
  border: 1px solid var(--border-subtle);
  border-radius: calc(var(--radius) + 2px);
  background: var(--surface-subtle);
}
.revisions-section.is-open {
  background: var(--surface);
}
.revisions-heading {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 66px;
  padding: 8px 10px;
}
.revisions-disclosure {
  display: grid;
  grid-template-columns: 38px minmax(0, 1fr) auto;
  flex: 1;
  align-items: center;
  gap: 11px;
  min-width: 0;
  padding: 5px;
  border: 0;
  background: transparent;
  text-align: left;
  cursor: pointer;
}
.revisions-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: 10px;
  background: var(--surface);
  color: var(--text-muted);
}
.is-open .revisions-icon {
  background: var(--accent-soft);
  color: var(--accent);
}
.revisions-heading-copy {
  display: grid;
  gap: 3px;
  min-width: 0;
}
.revisions-title-line {
  display: flex;
  align-items: center;
  gap: 7px;
}
.revisions-title-line strong {
  color: var(--text);
  font-size: 13px;
}
.revisions-heading-copy small {
  overflow: hidden;
  color: var(--text-muted);
  font-size: 10.5px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.revision-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 20px;
  height: 20px;
  padding: 0 6px;
  border-radius: 999px;
  background: var(--surface-muted);
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 700;
}
.revisions-chevron {
  color: var(--text-faint);
  transition: transform 150ms ease;
}
.is-open .revisions-chevron {
  transform: rotate(90deg);
}
.revisions-heading-actions {
  display: flex;
  align-items: center;
  gap: 5px;
}
.revisions-content {
  display: grid;
  gap: 14px;
  padding: 14px;
  border-top: 1px solid var(--border-subtle);
}
.revision-callout {
  margin: 0;
}
.revision-compare-panel {
  display: grid;
  gap: 12px;
  padding: 13px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}
.revision-compare-heading {
  display: flex;
  align-items: center;
  gap: 9px;
}
.revision-compare-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: var(--accent-soft);
  color: var(--accent);
}
.revision-compare-heading > div {
  display: grid;
  gap: 2px;
}
.revision-compare-heading strong {
  color: var(--text);
  font-size: 12px;
}
.revision-compare-heading small {
  color: var(--text-muted);
  font-size: 10.5px;
}
.revision-compare-fields {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 24px minmax(0, 1fr);
  align-items: end;
  gap: 8px;
}
.revision-compare-fields label {
  display: grid;
  gap: 5px;
  min-width: 0;
}
.revision-compare-fields label > span {
  color: var(--text-subtle);
  font-size: 11px;
  font-weight: 650;
}
.revision-compare-arrow {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 34px;
  color: var(--text-faint);
  transform: rotate(-90deg);
}
.revision-compare-hint {
  margin: 0;
  color: var(--warning-fg);
  font-size: 10.5px;
}
.revision-list {
  display: grid;
  gap: 8px;
}
.revision-card {
  display: grid;
  grid-template-columns: 54px minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
}
.revision-card.is-current {
  border-color: color-mix(in srgb, var(--accent) 30%, var(--border-subtle));
  background: color-mix(in srgb, var(--accent-soft) 20%, var(--surface));
}
.revision-number {
  display: grid;
  justify-items: center;
  gap: 1px;
  padding: 5px;
  border-right: 1px solid var(--border-subtle);
}
.revision-number span {
  color: var(--text-faint);
  font-size: 8px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.revision-number strong {
  color: var(--text);
  font-size: 19px;
  line-height: 1;
}
.revision-card-content {
  display: grid;
  gap: 5px;
  min-width: 0;
}
.revision-card-title {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
.revision-card-title > strong {
  color: var(--text);
  font-size: 12px;
}
.current-tag,
.source-tag {
  display: inline-flex;
  align-items: center;
  min-height: 19px;
  padding: 1px 6px;
  border-radius: 999px;
  font-size: 9px;
  font-weight: 700;
}
.current-tag {
  background: var(--success-bg);
  color: var(--success-fg);
}
.source-tag {
  background: var(--surface-muted);
  color: var(--text-muted);
}
.source-pack,
.source-rollback {
  background: var(--accent-soft);
  color: var(--accent-text);
}
.revision-note {
  margin: 0;
  color: var(--text-subtle);
  font-size: 11px;
}
.revision-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  color: var(--text-muted);
  font-size: 10.5px;
}
.revision-meta span {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.revision-card-action {
  display: flex;
  justify-content: flex-end;
}
.revision-current-label {
  color: var(--text-faint);
  font-size: 10.5px;
}
@media (max-width: 640px) {
  .revision-compare-fields {
    grid-template-columns: minmax(0, 1fr);
  }
  .revision-compare-arrow {
    height: auto;
    transform: none;
  }
  .revision-card {
    grid-template-columns: 48px minmax(0, 1fr);
  }
  .revision-card-action {
    grid-column: 2;
    justify-content: flex-start;
  }
  .revisions-heading-copy small {
    white-space: normal;
  }
}
</style>
