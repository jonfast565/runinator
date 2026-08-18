<template>
  <div class="retry-policy-editor">
    <div class="form-grid">
      <label>
        Max Attempts
        <input
          :value="policy.max_attempts"
          type="number"
          min="1"
          @input="setNumber('max_attempts', $event, 1)"
        />
        <small class="text-fg-muted">1 = never retried.</small>
      </label>
      <label>
        Retry On
        <select
          :value="policy.retry_on"
          :disabled="!retries"
          @change="setRetryOn(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="entry in RETRY_CLASSES" :key="entry.value" :value="entry.value">
            {{ entry.label }}
          </option>
        </select>
        <small class="text-fg-muted">{{ retryClassDescription }}</small>
      </label>
    </div>

    <!-- the backoff only means anything once there is a second attempt to delay. -->
    <div v-if="retries" class="form-grid">
      <label>
        Backoff Base (s)
        <input
          :value="policy.backoff_base_seconds"
          type="number"
          min="0"
          @input="setNumber('backoff_base_seconds', $event, 0)"
        />
        <small class="text-fg-muted">First delay; doubles each attempt.</small>
      </label>
      <label>
        Backoff Max (s)
        <input
          :value="policy.backoff_max_seconds"
          type="number"
          min="0"
          @input="setNumber('backoff_max_seconds', $event, 0)"
        />
        <small class="text-fg-muted">Ceiling on the doubling.</small>
      </label>
      <label class="checkbox">
        <input
          :checked="policy.jitter"
          type="checkbox"
          @change="setJitter(($event.target as HTMLInputElement).checked)"
        />
        Jitter
      </label>
    </div>

    <p class="retry-summary">{{ summary }}</p>
    <ul v-if="delays.length" class="retry-schedule">
      <li v-for="(delay, index) in delays" :key="index">
        <span>Attempt {{ index + 2 }}</span>
        <strong>after {{ formatDuration(delay) }}</strong>
      </li>
    </ul>
    <p v-if="delays.length" class="retry-window">
      Worst case the node spends {{ formatDuration(window) }} waiting between attempts, on top of
      the runs themselves.
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import {
  RETRY_CLASSES,
  describeRetryPolicy,
  formatDuration,
  retryDelays,
  retryWindowSeconds,
  type RetryPolicy,
} from "../../../core/workflow/retry";

const props = defineProps<{ modelValue: RetryPolicy }>();
const emit = defineEmits<(e: "update:modelValue", value: RetryPolicy) => void>();

const policy = computed(() => props.modelValue);
const retries = computed(() => policy.value.max_attempts > 1);
const delays = computed(() => retryDelays(policy.value));
const window = computed(() => retryWindowSeconds(policy.value));
const summary = computed(() => describeRetryPolicy(policy.value));
const retryClassDescription = computed(
  () => RETRY_CLASSES.find((entry) => entry.value === policy.value.retry_on)?.description ?? "",
);

function setNumber(key: keyof RetryPolicy, event: Event, min: number) {
  const raw = Number((event.target as HTMLInputElement).value);
  const value = Number.isFinite(raw) ? Math.max(min, Math.floor(raw)) : min;
  emit("update:modelValue", { ...policy.value, [key]: value });
}

function setRetryOn(value: string) {
  emit("update:modelValue", { ...policy.value, retry_on: value });
}

function setJitter(value: boolean) {
  emit("update:modelValue", { ...policy.value, jitter: value });
}
</script>

<style scoped>
.retry-policy-editor {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.retry-summary {
  margin: 0;
  font-size: 0.8rem;
}

.retry-schedule {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.retry-schedule li {
  display: flex;
  gap: 0.35rem;
  align-items: baseline;
  border: 1px solid var(--color-border, #3334);
  border-radius: 0.4rem;
  padding: 0.15rem 0.45rem;
  font-size: 0.72rem;
}

.retry-window {
  margin: 0;
  font-size: 0.72rem;
  opacity: 0.75;
}
</style>
