<template>
  <div class="typed-parameter-editor">
    <p v-if="parameters.length === 0" class="hint">This action does not publish typed parameters yet.</p>
    <div v-for="parameter in parameters" :key="parameter.name" class="parameter-row">
      <div class="parameter-field">
        <span class="parameter-label">
          {{ parameter.label || parameter.name }}
          <strong v-if="parameter.required">*</strong>
          <small>{{ typeLabel(parameter) }}</small>
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
        <TypedValueEditor
          v-else
          :model-value="parameterValue(parameter.name)"
          :ty="parameter.ty"
          :placeholder="placeholder(parameter)"
          :force-expression="isWorkflowExpressionValue(parameterValue(parameter.name))"
          :expression-context="expressionContext"
          @update:model-value="setValue(parameter.name, $event)"
        />
      </div>
      <p v-if="parameter.description" class="description">{{ parameter.description }}</p>
      <ul v-if="typeRows(parameter).length" class="type-rows">
        <li v-for="row in typeRows(parameter)" :key="row.path">
          <code>{{ row.path }}</code>
          <span>{{ row.required ? "required" : "optional" }}</span>
          <small>{{ row.type }}</small>
        </li>
      </ul>
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
import type { ActionParameterMetadata, CredentialSummary, JsonRecord, RuninatorType } from "../../types/models";
import type { WorkflowExpressionEditorContext } from "../../utils/workflow-expression-completion";
import { isWorkflowExpressionValue } from "../../utils/workflow-expression-completion";
import { parseSecretRef, secretRef, secretRefLabel } from "../../utils/secrets";
import { isBlankValue } from "../../utils/values";
import { useSecretsStore } from "../../stores/secrets";
import TypedValueEditor from "./TypedValueEditor.vue";

const props = defineProps<{
  modelValue: JsonRecord;
  parameters: ActionParameterMetadata[];
  credentialScopes?: string[];
  expressionContext?: WorkflowExpressionEditorContext;
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
    if (isEmpty(value)) continue;
    const typeError = validateValueType(value, parameter.ty, parameter.label || parameter.name);
    if (typeError) result[parameter.name] = typeError;
  }
  return result;
});

function setValue(name: string, value: unknown) {
  emit("update:modelValue", { ...props.modelValue, [name]: value });
}

function parameterValue(name: string): unknown {
  return props.modelValue[name];
}

function isString(parameter: ActionParameterMetadata): boolean {
  return typeKind(parameter) === "string";
}

function isSecretString(parameter: ActionParameterMetadata): boolean {
  return parameter.secret && isString(parameter);
}

function typeKind(parameter: ActionParameterMetadata): string {
  return parameter.ty?.type ?? "any";
}

function typeLabel(parameter: ActionParameterMetadata): string {
  return describeType(parameter.ty);
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

function placeholder(parameter: ActionParameterMetadata): string {
  if (parameter.default_value === undefined || parameter.default_value === null) return "";
  return String(parameter.default_value);
}

function isEmpty(value: unknown): boolean {
  return isBlankValue(value);
}

function describeType(ty: RuninatorType | undefined, depth = 0): string {
  if (!ty) return "any";
  if (ty.type === "array") return `${describeType(ty.items, depth + 1)}[]`;
  if (ty.type === "map") return `map<string, ${describeType(ty.values, depth + 1)}>`;
  if (ty.type === "union") return ty.variants.map((variant) => describeType(variant, depth + 1)).join(" | ");
  if (ty.type !== "struct") return ty.type;
  const entries = Object.entries(ty.fields);
  if (depth > 0 || entries.length > 3) return "struct";
  const fields = entries
    .map(([name, field]) => `${name}${field.required ? "" : "?"}: ${describeType(field.ty, depth + 1)}`)
    .join("; ");
  return `{ ${fields} }`;
}

function typeRows(parameter: ActionParameterMetadata): Array<{ path: string; required: boolean; type: string }> {
  const rows: Array<{ path: string; required: boolean; type: string }> = [];
  collectTypeRows(parameter.ty, parameter.name, parameter.required, rows);
  return rows.slice(1, 9);
}

function collectTypeRows(
  ty: RuninatorType | undefined,
  path: string,
  required: boolean,
  rows: Array<{ path: string; required: boolean; type: string }>
) {
  if (!ty) return;
  rows.push({ path, required, type: describeType(ty, 1) });
  if (ty.type === "array") {
    collectTypeRows(ty.items, `${path}[]`, true, rows);
    return;
  }
  if (ty.type === "map") {
    collectTypeRows(ty.values, `${path}.*`, true, rows);
    return;
  }
  if (ty.type !== "struct") return;
  for (const [name, field] of Object.entries(ty.fields)) {
    collectTypeRows(field.ty, `${path}.${name}`, field.required, rows);
  }
  if (ty.additional) collectTypeRows(ty.additional, `${path}.*`, true, rows);
}

function validateValueType(value: unknown, ty: RuninatorType | undefined, label: string): string {
  if (!ty || ty.type === "any") return "";
  if (isExpressionValue(value)) return "";
  if (ty.type === "null") return value === null ? "" : `${label} must be null`;
  if (ty.type === "string") return typeof value === "string" ? "" : `${label} must be a string`;
  if (ty.type === "boolean") return typeof value === "boolean" ? "" : `${label} must be true or false`;
  if (ty.type === "integer") return typeof value === "number" && Number.isInteger(value) ? "" : `${label} must be an integer`;
  if (ty.type === "number") return typeof value === "number" && !Number.isNaN(value) ? "" : `${label} must be a number`;
  if (ty.type === "array") {
    if (!Array.isArray(value)) return `${label} must be a list`;
    for (let i = 0; i < value.length; i++) {
      const error = validateValueType(value[i], ty.items, `${label}[${i}]`);
      if (error) return error;
    }
    return "";
  }
  if (ty.type === "map") {
    if (!isPlainRecord(value)) return `${label} must be an object`;
    for (const [key, nested] of Object.entries(value)) {
      const error = validateValueType(nested, ty.values, `${label}.${key}`);
      if (error) return error;
    }
    return "";
  }
  if (ty.type === "struct") {
    if (!isPlainRecord(value)) return `${label} must be an object`;
    for (const [key, field] of Object.entries(ty.fields)) {
      const nested = value[key];
      if (isEmpty(nested)) {
        if (field.required) return `${label}.${key} is required`;
        continue;
      }
      const error = validateValueType(nested, field.ty, `${label}.${key}`);
      if (error) return error;
    }
    for (const [key, nested] of Object.entries(value)) {
      if (ty.fields[key]) continue;
      if (!ty.additional) return `${label}.${key} is not allowed`;
      const error = validateValueType(nested, ty.additional, `${label}.${key}`);
      if (error) return error;
    }
    return "";
  }
  if (ty.type === "union") {
    return ty.variants.some((variant) => !validateValueType(value, variant, label))
      ? ""
      : `${label} must match ${describeType(ty)}`;
  }
  return "";
}

function isPlainRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isExpressionValue(value: unknown): boolean {
  return isWorkflowExpressionValue(value);
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

.parameter-field {
  display: grid;
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
.type-rows {
  display: grid;
  gap: 2px;
  list-style: none;
  margin: 2px 0 0;
  padding: 0;
}
.type-rows li {
  display: grid;
  grid-template-columns: minmax(120px, 1fr) auto minmax(80px, 0.8fr);
  gap: 8px;
  align-items: baseline;
  color: #66717e;
  font-size: 12px;
}
.type-rows code {
  color: #2f3a45;
  font-size: 12px;
}
.error {
  color: #c53030;
  font-size: 12px;
  margin: 0;
}
</style>
