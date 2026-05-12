<template>
  <div class="typed-parameter-editor">
    <p v-if="parameters.length === 0" class="hint">This action does not publish typed parameters yet.</p>
    <div v-for="parameter in parameters" :key="parameter.name" class="parameter-row">
      <label>
        <span class="parameter-label">
          {{ parameter.label || parameter.name }}
          <strong v-if="parameter.required">*</strong>
          <small>{{ parameter.value_type }}</small>
        </span>
        <select
          v-if="isSecretString(parameter)"
          :value="stringValue(parameter.name)"
          @change="setValue(parameter.name, ($event.target as HTMLSelectElement).value)"
        >
          <option value="" disabled>{{ secretOptions.length ? "Select secret" : "No secrets available" }}</option>
          <option v-if="currentSecretValueMissing(parameter.name)" :value="stringValue(parameter.name)">
            {{ currentSecretLabel(parameter.name) }}
          </option>
          <option v-for="secret in secretOptions" :key="`${secret.scope}:${secret.name}`" :value="secretOptionValue(secret)">
            {{ secret.scope }}/{{ secret.name }}
          </option>
        </select>
        <input
          v-else-if="isString(parameter)"
          :type="parameter.secret ? 'password' : 'text'"
          :value="stringValue(parameter.name)"
          :placeholder="placeholder(parameter)"
          @input="setValue(parameter.name, ($event.target as HTMLInputElement).value)"
        />
        <input
          v-else-if="parameter.value_type === 'integer' || parameter.value_type === 'number'"
          type="number"
          :step="parameter.value_type === 'integer' ? 1 : 'any'"
          :value="numberValue(parameter.name)"
          @input="setNumberValue(parameter, ($event.target as HTMLInputElement).value)"
        />
        <input
          v-else-if="parameter.value_type === 'boolean'"
          type="checkbox"
          :checked="Boolean(modelValue[parameter.name])"
          @change="setValue(parameter.name, ($event.target as HTMLInputElement).checked)"
        />
        <textarea
          v-else-if="parameter.value_type === 'string_array'"
          :value="arrayText(parameter.name)"
          placeholder="one value per line"
          @input="setValue(parameter.name, splitLines(($event.target as HTMLTextAreaElement).value))"
        />
        <JsonEditor
          v-else
          :model-value="jsonText(parameter.name)"
          @update:model-value="setJsonValue(parameter.name, $event)"
        />
      </label>
      <p v-if="parameter.description" class="description">{{ parameter.description }}</p>
      <p v-if="errors[parameter.name]" class="error">{{ errors[parameter.name] }}</p>
    </div>
    <p class="hint">
      Workflow parameters can reference prior results with tagged refs like
      <code>{ "$ref": { "prev": ["ticket_id"] } }</code> or named steps like
      <code>{ "$ref": { "node": "create_ticket", "output": ["ticket_id"] } }</code>.
      Secret parameters use <code>secret://scope/name</code> references.
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from "vue";
import type { ActionParameterMetadata, CredentialSummary, JsonRecord } from "../../types/models";
import { pretty } from "../../utils/format";
import { parseSecretRef, secretRef, secretRefLabel } from "../../utils/secrets";
import { useSecretsStore } from "../../stores/secrets";
import JsonEditor from "./JsonEditor.vue";

const props = defineProps<{
  modelValue: JsonRecord;
  parameters: ActionParameterMetadata[];
  credentialScopes?: string[];
}>();

const emit = defineEmits<{
  "update:modelValue": [value: JsonRecord];
}>();

const secrets = useSecretsStore();
const secretOptions = computed(() => secrets.secretsForScopes(props.credentialScopes ?? []));

onMounted(() => {
  if (secrets.secrets.length === 0) secrets.refreshSecrets();
});

const errors = computed(() => {
  const result: Record<string, string> = {};
  for (const parameter of props.parameters) {
    const value = props.modelValue[parameter.name];
    if (parameter.required && isEmpty(value)) {
      result[parameter.name] = "Required";
      continue;
    }
    if (value === undefined || value === null || value === "") continue;
    if (parameter.value_type === "integer" && !Number.isInteger(Number(value))) {
      result[parameter.name] = "Must be an integer";
    }
    if (parameter.value_type === "number" && Number.isNaN(Number(value))) {
      result[parameter.name] = "Must be a number";
    }
    if (parameter.value_type === "string_array" && !Array.isArray(value)) {
      result[parameter.name] = "Must be a string array";
    }
  }
  return result;
});

function setValue(name: string, value: unknown) {
  emit("update:modelValue", { ...props.modelValue, [name]: value });
}

function setNumberValue(parameter: ActionParameterMetadata, raw: string) {
  if (raw.trim() === "") {
    setValue(parameter.name, null);
    return;
  }
  const value = parameter.value_type === "integer" ? Number.parseInt(raw, 10) : Number(raw);
  setValue(parameter.name, value);
}

function setJsonValue(name: string, raw: string) {
  try {
    setValue(name, JSON.parse(raw || "null"));
  } catch {
    setValue(name, raw);
  }
}

function isString(parameter: ActionParameterMetadata): boolean {
  return parameter.value_type === "string";
}

function isSecretString(parameter: ActionParameterMetadata): boolean {
  return parameter.secret && isString(parameter);
}

function stringValue(name: string): string {
  const value = props.modelValue[name];
  return typeof value === "string" ? value : "";
}

function secretOptionValue(secret: CredentialSummary): string {
  return secretRef(secret.scope, secret.name);
}

function currentSecretValueMissing(name: string): boolean {
  const value = stringValue(name);
  if (!value) return false;
  return !secretOptions.value.some((secret) => secretOptionValue(secret) === value);
}

function currentSecretLabel(name: string): string {
  const value = stringValue(name);
  const parsed = parseSecretRef(value);
  if (parsed) return `Missing secret: ${secretRefLabel(value)}`;
  return "Existing literal secret value";
}

function numberValue(name: string): string | number {
  const value = props.modelValue[name];
  return typeof value === "number" ? value : "";
}

function arrayText(name: string): string {
  const value = props.modelValue[name];
  return Array.isArray(value) ? value.join("\n") : "";
}

function splitLines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function jsonText(name: string): string {
  return pretty(props.modelValue[name] ?? {});
}

function placeholder(parameter: ActionParameterMetadata): string {
  if (parameter.default_value === undefined || parameter.default_value === null) return "";
  return String(parameter.default_value);
}

function isEmpty(value: unknown): boolean {
  return value === undefined || value === null || value === "" || (Array.isArray(value) && value.length === 0);
}
</script>

<style scoped>
.typed-parameter-editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.parameter-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.parameter-label {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  align-items: baseline;
}
.parameter-label small,
.hint,
.description {
  color: #66717e;
  font-size: 12px;
}
.description {
  margin: 0;
}
.error {
  color: #c53030;
  font-size: 12px;
  margin: 0;
}
</style>
