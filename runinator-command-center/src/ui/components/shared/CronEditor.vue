<template>
  <div class="cron-editor">
    <div class="cron-top">
      <label class="cron-preset">
        <span>Schedule</span>
        <select
          :value="presetId"
          @change="selectPreset(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="preset in CRON_PRESETS" :key="preset.id" :value="preset.id">
            {{ preset.label }}
          </option>
          <option :value="CUSTOM_PRESET_ID">Custom…</option>
        </select>
      </label>
      <label class="cron-raw-toggle checkbox">
        <input type="checkbox" :checked="rawMode" @change="toggleRaw" />
        Raw expression
      </label>
    </div>

    <!-- Most schedules are completely described by a preset. Keep the five-field builder nearby,
         without making every operator parse it before they can move on. -->
    <details
      v-if="!rawMode && fields"
      class="cron-fine-tune"
      :open="fineTuneOpen || presetId === CUSTOM_PRESET_ID"
      @toggle="fineTuneOpen = ($event.target as HTMLDetailsElement).open"
    >
      <summary>
        <span>Fine-tune cron fields</span>
        <code>{{ modelValue }}</code>
      </summary>
      <div class="cron-fields">
        <label v-for="name in CRON_FIELD_ORDER" :key="name" class="cron-field">
          <span>{{ cronFieldLabel(name) }}</span>
          <div class="cron-field-control">
            <input
              :value="fields[name]"
              :class="{ invalid: fieldError(name) }"
              spellcheck="false"
              @input="setField(name, ($event.target as HTMLInputElement).value)"
            />
            <select class="cron-field-pick" :value="''" @change="pickField(name, $event)">
              <option value="">Every</option>
              <option
                v-for="option in cronFieldOptions(name)"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </div>
          <small v-if="fieldError(name)" class="cron-field-error">{{ fieldError(name) }}</small>
        </label>
      </div>
    </details>

    <!-- raw mode, and the only mode for the six-field and @alias forms the builder cannot model. -->
    <label v-else class="cron-raw">
      <span>Expression</span>
      <input
        :value="modelValue"
        placeholder="0 * * * *"
        spellcheck="false"
        @input="emitExpression(($event.target as HTMLInputElement).value)"
      />
    </label>

    <div v-if="showDetails" class="cron-summary">
      <code class="cron-expression">{{ modelValue || "(empty)" }}</code>
      <p v-if="error" class="cron-error">{{ error }}</p>
      <template v-else>
        <p v-if="summary" class="cron-description">{{ summary }}</p>
        <p v-else class="cron-description muted">
          Not a five-field expression; the backend still parses it.
        </p>
        <ul v-if="upcoming.length" class="cron-next">
          <li v-for="run in upcoming" :key="run">{{ run }}</li>
        </ul>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import {
  CRON_FIELD_ORDER,
  CRON_PRESETS,
  CUSTOM_PRESET_ID,
  cronFieldLabel,
  cronFieldOptions,
  describeCron,
  emptyCronFields,
  formatCronRun,
  joinCron,
  matchCronPreset,
  nextCronRuns,
  splitCron,
  validateCron,
  validateCronField,
  type CronFieldName,
} from "../../../core/workflow/cron";

const props = withDefaults(defineProps<{ modelValue: string; showDetails?: boolean }>(), {
  showDetails: true,
});
const emit = defineEmits<(e: "update:modelValue", value: string) => void>();

// an expression the builder cannot represent (six-field, `@daily`) has to be edited as text, so raw
// mode is forced there rather than offered — dropping into the builder would rewrite it.
const rawRequested = ref(false);
const fineTuneOpen = ref(false);
const fields = computed(() => splitCron(props.modelValue));
const rawMode = computed(() => rawRequested.value || fields.value === null);

const presetId = computed(() => matchCronPreset(props.modelValue));
const error = computed(() => (props.modelValue.trim() ? validateCron(props.modelValue) : null));
const summary = computed(() => describeCron(props.modelValue));
const upcoming = computed(() => nextCronRuns(props.modelValue, 3).map(formatCronRun));

function emitExpression(value: string) {
  emit("update:modelValue", value);
}

function toggleRaw() {
  rawRequested.value = !rawRequested.value;
}

function selectPreset(id: string) {
  const preset = CRON_PRESETS.find((entry) => entry.id === id);

  if (!preset) {
    // "Custom…" is a destination, not a value: keep whatever is there and open the builder on it.
    rawRequested.value = false;

    if (!splitCron(props.modelValue)) {
      emitExpression(joinCron(emptyCronFields()));
    }

    return;
  }

  rawRequested.value = false;
  emitExpression(preset.expression);
}

function setField(name: CronFieldName, value: string) {
  const current = fields.value;

  if (!current) {
    return;
  }

  emitExpression(joinCron({ ...current, [name]: value }));
}

// the per-field dropdown is an insert, not a binding: it appends the picked value to the field so a
// list like `1,3,5` can be built without typing, and resets so the same value can be picked twice.
function pickField(name: CronFieldName, event: Event) {
  const select = event.target as HTMLSelectElement;
  const picked = select.value;
  select.value = "";
  const current = fields.value;

  if (!current) {
    return;
  }

  if (!picked) {
    setField(name, "*");
    return;
  }

  const existing = current[name].trim();
  const parts = existing === "*" || existing === "" ? [] : existing.split(",");

  if (parts.includes(picked)) {
    return;
  }

  setField(name, [...parts, picked].join(","));
}

function fieldError(name: CronFieldName): string | null {
  const current = fields.value;
  return current ? validateCronField(current[name], name) : null;
}
</script>

<style scoped>
.cron-editor {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.cron-top {
  display: flex;
  flex-wrap: wrap;
  align-items: end;
  gap: 0.75rem;
}

.cron-preset {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: min(24rem, 100%);
  flex: 1;
}

.cron-fine-tune {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface-subtle);
}

.cron-fine-tune > summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.65rem 0.75rem;
  color: var(--text-subtle);
  font-size: 0.75rem;
  font-weight: 650;
  cursor: pointer;
}

.cron-fine-tune > summary code {
  color: var(--text-muted);
  font-size: 0.7rem;
  font-weight: 500;
}

.cron-fine-tune[open] > summary {
  border-bottom: 1px solid var(--border-subtle);
}

.cron-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.75rem;
  padding: 0.75rem;
}

.cron-field,
.cron-raw {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
}

.cron-field > span,
.cron-raw > span,
.cron-preset > span {
  color: var(--text-subtle);
  font-size: 0.75rem;
  font-weight: 650;
}

.cron-field-control {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(7rem, 0.65fr);
  gap: 0.4rem;
}

.cron-field-pick {
  font-size: 0.75rem;
}

.cron-field input.invalid {
  border-color: var(--color-danger-fg, #d33);
}

.cron-field-error,
.cron-error {
  color: var(--color-danger-fg, #d33);
  font-size: 0.72rem;
}

.cron-summary {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.cron-expression {
  font-size: 0.85rem;
}

.cron-description {
  margin: 0;
  font-size: 0.78rem;
}

.cron-description.muted {
  opacity: 0.7;
}

.cron-next {
  margin: 0;
  padding-left: 1rem;
  font-size: 0.72rem;
  opacity: 0.8;
}

@media (max-width: 640px) {
  .cron-top,
  .cron-fine-tune > summary {
    align-items: stretch;
    flex-direction: column;
  }

  .cron-fields {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
