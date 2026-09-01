<template>
  <section class="schedule-editor rounded-lg border border-border bg-surface-raised p-4">
    <div class="mb-4 flex flex-wrap items-start justify-between gap-3">
      <div>
        <h4 class="m-0 text-sm font-semibold">{{ title }}</h4>
        <p class="m-0 mt-0.5 text-xs text-fg-muted">{{ description }}</p>
      </div>
      <span class="rounded-full bg-accent-soft px-2 py-1 text-[11px] text-accent">{{
        summary
      }}</span>
    </div>

    <div class="mb-4 grid grid-cols-2 gap-1.5 rounded-md bg-surface p-1.5 sm:grid-cols-4">
      <button
        v-for="option in kinds"
        :key="option.kind"
        type="button"
        class="rounded px-3 py-2 text-xs font-medium transition"
        :class="
          modelValue.recurrence.kind === option.kind
            ? 'bg-accent text-white shadow-sm'
            : 'text-fg-muted hover:bg-surface-hover'
        "
        @click="setKind(option.kind)"
      >
        {{ option.label }}
      </button>
    </div>

    <div class="grid gap-4">
      <label v-if="modelValue.recurrence.kind === 'once'" class="schedule-field">
        <span>Occurs at</span>
        <input
          type="datetime-local"
          :value="onceLocal"
          @input="setOnce(($event.target as HTMLInputElement).value)"
        />
      </label>

      <CronEditor
        v-else-if="modelValue.recurrence.kind === 'cron'"
        :model-value="modelValue.recurrence.expression"
        :show-details="false"
        @update:model-value="setCron"
      />

      <template v-else-if="modelValue.recurrence.kind === 'weekdays'">
        <div class="schedule-field">
          <span>Days of the week</span>
          <div class="flex flex-wrap gap-1.5">
            <button
              v-for="day in SCHEDULE_WEEKDAYS"
              :key="day.value"
              type="button"
              class="min-w-10 rounded-full border px-2.5 py-1 text-xs"
              :class="
                modelValue.recurrence.days.includes(day.value)
                  ? 'border-accent bg-accent-soft text-accent'
                  : 'border-border text-fg-muted hover:border-accent/50'
              "
              :title="day.label"
              @click="toggleDay(day.value)"
            >
              {{ day.short }}
            </button>
          </div>
          <div class="mt-1 flex gap-1">
            <button
              type="button"
              class="btn btn-ghost !px-2 !py-1 text-xs"
              @click="setDays('weekdays')"
            >
              Weekdays
            </button>
            <button
              type="button"
              class="btn btn-ghost !px-2 !py-1 text-xs"
              @click="setDays('weekends')"
            >
              Weekends
            </button>
            <button
              type="button"
              class="btn btn-ghost !px-2 !py-1 text-xs"
              @click="setDays('daily')"
            >
              Every day
            </button>
          </div>
        </div>
        <label class="schedule-field max-w-48">
          <span>Local start time</span>
          <input
            type="time"
            :value="weekdayTime"
            step="60"
            @input="setWeekdayTime(($event.target as HTMLInputElement).value)"
          />
        </label>
      </template>

      <template v-else>
        <div class="schedule-field">
          <span>RRULE template</span>
          <div class="flex flex-wrap gap-1.5">
            <button
              v-for="template in RRULE_TEMPLATES"
              :key="template.label"
              type="button"
              class="btn btn-ghost !px-2 !py-1 text-xs"
              @click="setRrule(template.rule)"
            >
              {{ template.label }}
            </button>
          </div>
        </div>
        <label class="schedule-field">
          <span>RFC 5545 RRULE</span>
          <textarea
            :value="modelValue.recurrence.rule"
            rows="2"
            spellcheck="false"
            placeholder="FREQ=WEEKLY;BYDAY=TU,WE"
            @input="setRrule(($event.target as HTMLTextAreaElement).value)"
          />
          <small>Enter the rule body; DTSTART and timezone are managed separately.</small>
        </label>
        <label class="schedule-field max-w-64">
          <span>DTSTART</span>
          <input
            type="datetime-local"
            :value="rruleStartLocal"
            @input="setRruleStart(($event.target as HTMLInputElement).value)"
          />
        </label>
      </template>

      <div class="grid gap-3" :class="{ 'sm:grid-cols-2': window }">
        <label class="schedule-field">
          <span>Timezone</span>
          <input v-model="timezoneModel" list="schedule-timezones" placeholder="America/New_York" />
          <datalist id="schedule-timezones">
            <option v-for="zone in timezones" :key="zone" :value="zone" />
          </datalist>
          <small>Uses IANA timezone rules, including daylight-saving transitions.</small>
        </label>
        <label v-if="window" class="schedule-field">
          <span>Window duration</span>
          <div class="flex items-center gap-2">
            <input
              class="min-w-0 flex-1"
              type="number"
              min="1"
              step="1"
              :value="durationMinutes"
              @input="setDuration(($event.target as HTMLInputElement).value)"
            />
            <span class="text-xs text-fg-muted">minutes</span>
          </div>
        </label>
      </div>
    </div>

    <p
      v-if="error"
      class="mb-0 mt-3 rounded bg-danger-soft px-2 py-1.5 text-xs text-danger"
      role="alert"
    >
      {{ error }}
    </p>
    <div v-else class="mt-4 border-t border-border pt-3">
      <div class="flex items-center justify-between gap-2">
        <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-muted"
          >Upcoming</span
        >
        <span
          v-if="modelValue.recurrence.kind === 'cron' && modelValue.timezone !== 'UTC'"
          class="text-[11px] text-fg-muted"
          >Saved schedule uses {{ modelValue.timezone }}</span
        >
      </div>
      <ol v-if="preview.length" class="mt-2 grid gap-2 text-xs sm:grid-cols-2">
        <li
          v-for="(date, index) in preview"
          :key="date.toISOString()"
          class="schedule-preview-item"
        >
          <span>{{ index === 0 ? "Next" : `Then ${String(index)}` }}</span>
          <strong>{{ formatOccurrence(date, modelValue.timezone) }}</strong>
        </li>
      </ol>
      <p v-else class="mb-0 mt-1 text-xs text-fg-muted">
        Preview becomes available when the recurrence is complete.
      </p>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type {
  ScheduleRecurrence,
  ScheduleSpec,
  ScheduleWeekday,
} from "../../../core/domain/models";
import {
  RRULE_TEMPLATES,
  SCHEDULE_WEEKDAYS,
  browserTimezone,
  describeSchedule,
  formatOccurrence,
  previewSchedule,
  validateSchedule,
} from "../../../core/workflow/schedule";
import CronEditor from "./CronEditor.vue";

const props = withDefaults(
  defineProps<{
    modelValue: ScheduleSpec;
    window?: boolean;
    title?: string;
    description?: string;
  }>(),
  {
    window: false,
    title: "Schedule",
    description: "Choose when this schedule occurs.",
  },
);
const emit = defineEmits<(event: "update:modelValue", value: ScheduleSpec) => void>();
const kinds: { kind: ScheduleRecurrence["kind"]; label: string }[] = [
  { kind: "once", label: "Once" },
  { kind: "cron", label: "Cron" },
  { kind: "weekdays", label: "Weekdays" },
  { kind: "rrule", label: "RRULE" },
];
const timezones = Array.from(
  new Set([
    browserTimezone(),
    "UTC",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "Europe/London",
    "Europe/Paris",
    "Asia/Tokyo",
    "Australia/Sydney",
  ]),
);
const error = computed(() => validateSchedule(props.modelValue, props.window));
const summary = computed(() => describeSchedule(props.modelValue));
const preview = computed(() => previewSchedule(props.modelValue));
const durationMinutes = computed(() =>
  Math.max(1, Math.round(props.modelValue.duration_seconds / 60)),
);
const timezoneModel = computed({
  get: () => props.modelValue.timezone,
  set: (timezone: string) => {
    update({ timezone });
  },
});
const onceLocal = computed(() =>
  props.modelValue.recurrence.kind === "once" ? toLocal(props.modelValue.recurrence.at) : "",
);
const rruleStartLocal = computed(() =>
  props.modelValue.recurrence.kind === "rrule" ? toLocal(props.modelValue.recurrence.dtstart) : "",
);
const weekdayTime = computed(() =>
  props.modelValue.recurrence.kind === "weekdays"
    ? `${pad(props.modelValue.recurrence.hour)}:${pad(props.modelValue.recurrence.minute)}`
    : "09:00",
);

function update(partial: Partial<ScheduleSpec>) {
  emit("update:modelValue", { ...props.modelValue, ...partial });
}

function setKind(kind: ScheduleRecurrence["kind"]) {
  const now = new Date(Date.now() + 3_600_000).toISOString();
  const recurrence: ScheduleRecurrence =
    kind === "once"
      ? { kind, at: now }
      : kind === "cron"
        ? { kind, expression: "0 9 * * 1-5" }
        : kind === "weekdays"
          ? {
              kind,
              days: ["monday", "tuesday", "wednesday", "thursday", "friday"],
              hour: 9,
              minute: 0,
              second: 0,
            }
          : { kind, rule: "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR", dtstart: now };
  update({ recurrence });
}

function setOnce(value: string) {
  if (value) {
    update({ recurrence: { kind: "once", at: new Date(value).toISOString() } });
  }
}

function setCron(expression: string) {
  update({ recurrence: { kind: "cron", expression } });
}

function toggleDay(day: ScheduleWeekday) {
  if (props.modelValue.recurrence.kind !== "weekdays") {
    return;
  }

  const days = props.modelValue.recurrence.days.includes(day)
    ? props.modelValue.recurrence.days.filter((value) => value !== day)
    : [...props.modelValue.recurrence.days, day];
  update({ recurrence: { ...props.modelValue.recurrence, days } });
}

function setDays(preset: "weekdays" | "weekends" | "daily") {
  if (props.modelValue.recurrence.kind !== "weekdays") {
    return;
  }

  const days =
    preset === "weekdays"
      ? SCHEDULE_WEEKDAYS.slice(0, 5).map((day) => day.value)
      : preset === "weekends"
        ? SCHEDULE_WEEKDAYS.slice(5).map((day) => day.value)
        : SCHEDULE_WEEKDAYS.map((day) => day.value);
  update({ recurrence: { ...props.modelValue.recurrence, days } });
}

function setWeekdayTime(value: string) {
  if (props.modelValue.recurrence.kind !== "weekdays") {
    return;
  }

  const [hour, minute] = value.split(":").map(Number);
  update({ recurrence: { ...props.modelValue.recurrence, hour, minute, second: 0 } });
}

function setRrule(rule: string) {
  if (props.modelValue.recurrence.kind === "rrule") {
    update({ recurrence: { ...props.modelValue.recurrence, rule } });
  }
}

function setRruleStart(value: string) {
  if (value && props.modelValue.recurrence.kind === "rrule") {
    update({
      recurrence: { ...props.modelValue.recurrence, dtstart: new Date(value).toISOString() },
    });
  }
}

function setDuration(value: string) {
  update({ duration_seconds: Math.max(1, Number(value) || 1) * 60 });
}

function toLocal(value: string) {
  const date = new Date(value);
  return new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
}

function pad(value: number) {
  return String(value).padStart(2, "0");
}
</script>

<style scoped>
.schedule-field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
  font-size: 0.75rem;
  color: var(--color-fg-muted);
}
.schedule-field small {
  font-size: 0.68rem;
  color: var(--color-fg-muted);
}
.schedule-field textarea {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  resize: vertical;
}
.schedule-preview-item {
  display: grid;
  gap: 0.15rem;
  padding: 0.55rem 0.65rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.375rem;
  background: var(--color-surface);
}
.schedule-preview-item span {
  color: var(--color-fg-muted);
  font-size: 0.62rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.schedule-preview-item strong {
  color: var(--color-fg);
  font-size: 0.72rem;
  font-weight: 600;
}
</style>
