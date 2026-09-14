<template>
  <div class="grid gap-3">
    <div class="flex flex-wrap gap-2" role="radiogroup" aria-label="Authentication method">
      <label v-if="!authentication.required" class="checkbox">
        <input v-model="selectedMode" type="radio" value="none" @change="selectMode('none')" />
        None
      </label>
      <label
        v-for="(alternative, index) in authentication.alternatives"
        :key="modeValue(alternative, index)"
        class="checkbox"
      >
        <input
          v-model="selectedMode"
          type="radio"
          :value="modeValue(alternative, index)"
          @change="selectMode(modeValue(alternative, index))"
        />
        {{ alternativeLabel(alternative) }}
      </label>
    </div>

    <div v-if="selectedSecrets" class="grid gap-2">
      <label v-for="parameter in selectedSecrets.parameters" :key="parameter">
        <span>{{ parameterLabel(parameter) }}</span>
        <select :value="secretValue(parameter)" @change="setSecret(parameter, $event)">
          <option value="" disabled>Choose a stored secret</option>
          <option v-if="missingSecret(parameter)" :value="secretValue(parameter)">
            Existing or unavailable secret
          </option>
          <option
            v-for="secret in secretOptions"
            :key="`${secret.scope}/${secret.name}`"
            :value="secretRef(secret.scope, secret.name)"
          >
            {{ secret.scope }}/{{ secret.name }}
          </option>
        </select>
      </label>
    </div>

    <label v-if="selectedMode === 'execution_profile'">
      <span>Execution profile</span>
      <select :value="profileId" @change="setProfile(($event.target as HTMLSelectElement).value)">
        <option value="" disabled>Choose an execution profile</option>
        <option v-if="profileMissing" :value="profileId">Existing or unavailable profile</option>
        <option v-for="profile in compatibleProfiles" :key="profile.id" :value="profile.id">
          {{ profile.name }} · {{ profile.health }}
        </option>
      </select>
      <small
        >Profiles must provide: {{ credentialScopes.join(", ") || "no declared scopes" }}.</small
      >
    </label>

    <div
      v-if="
        authentication.allow_multiple && selectedMode === 'execution_profile' && combinableSecrets
      "
      class="grid gap-2"
    >
      <small
        >Optional secret override; injected values take precedence over profile credentials.</small
      >
      <label v-for="parameter in combinableSecrets.parameters" :key="`combined-${parameter}`">
        <span>{{ parameterLabel(parameter) }}</span>
        <select :value="secretValue(parameter)" @change="setSecret(parameter, $event)">
          <option value="">Use execution profile</option>
          <option v-if="missingSecret(parameter)" :value="secretValue(parameter)">
            Existing or unavailable secret
          </option>
          <option
            v-for="secret in secretOptions"
            :key="`combined-${secret.scope}/${secret.name}`"
            :value="secretRef(secret.scope, secret.name)"
          >
            {{ secret.scope }}/{{ secret.name }}
          </option>
        </select>
      </label>
    </div>

    <p v-if="authentication.required && selectedMode === 'none'" class="error">
      Choose an authentication method.
    </p>
    <p
      v-if="selectedMode === 'execution_profile' && selectedProfile && !profileUsable"
      class="hint warn"
    >
      This profile is currently {{ selectedProfile.health }}; it can be saved, but execution will
      wait until it is usable.
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import type {
  ActionAuthenticationAlternative,
  ActionAuthenticationMetadata,
  ActionMetadata,
  JsonRecord,
} from "../../../core/domain/models";
import { asJsonRecord } from "../../../core/domain/json";
import { secretRef } from "../../../core/utils/secrets";
import { useExecutionProfilesStore } from "../../adapters/pinia/executionProfiles";
import { useSecretsStore } from "../../adapters/pinia/secrets";

const props = defineProps<{
  modelValue: JsonRecord;
  action: ActionMetadata;
  authentication: ActionAuthenticationMetadata;
  credentialScopes: string[];
}>();

const emit = defineEmits<{ "update:modelValue": [value: JsonRecord] }>();
const secrets = useSecretsStore();
const profiles = useExecutionProfilesStore();

const allSecretParameters = computed(() =>
  props.authentication.alternatives.flatMap((alternative) =>
    alternative.kind === "secrets" ? alternative.parameters : [],
  ),
);
const actionObject = computed(() => asJsonRecord(props.modelValue));
const configuration = computed(() => asJsonRecord(actionObject.value.configuration));
const profileBinding = computed(() => asJsonRecord(actionObject.value.execution_profile));
const profileReference = computed(() => asJsonRecord(profileBinding.value.reference));
const profileId = computed(() => {
  const value = profileBinding.value.id ?? profileReference.value.id;
  return typeof value === "string" ? value : "";
});
const compatibleProfiles = computed(() =>
  profiles.profiles.filter(
    (profile) =>
      profile.enabled &&
      props.credentialScopes.every((scope) => profile.credential_scopes.includes(scope)),
  ),
);
const selectedProfile = computed(
  () => profiles.profiles.find((profile) => profile.id === profileId.value) ?? null,
);
const profileMissing = computed(
  () =>
    Boolean(profileId.value) &&
    !compatibleProfiles.value.some((profile) => profile.id === profileId.value),
);
const profileUsable = computed(() =>
  selectedProfile.value ? ["ready", "expiring"].includes(selectedProfile.value.health) : false,
);
const secretOptions = computed(() => secrets.secretsForScopes(props.credentialScopes));

function detectedMode(): string {
  if (profileId.value || Object.keys(profileBinding.value).length) {
    return "execution_profile";
  }

  const index = props.authentication.alternatives.findIndex(
    (alternative) =>
      alternative.kind === "secrets" &&
      alternative.parameters.some((parameter) => Boolean(configuration.value[parameter])),
  );
  return index >= 0 ? `secrets:${String(index)}` : "none";
}

const selectedMode = ref(detectedMode());
watch(
  () => props.action.function_name,
  () => {
    selectedMode.value = detectedMode();
  },
);

const selectedSecrets = computed(() => {
  const index = Number(selectedMode.value.split(":")[1]);
  const alternative = props.authentication.alternatives.at(index);

  return alternative?.kind === "secrets" ? alternative : null;
});
const combinableSecrets = computed(
  () =>
    props.authentication.alternatives.find((alternative) => alternative.kind === "secrets") ?? null,
);

function modeValue(alternative: ActionAuthenticationAlternative, index: number): string {
  return alternative.kind === "execution_profile"
    ? "execution_profile"
    : `secrets:${String(index)}`;
}

function alternativeLabel(alternative: ActionAuthenticationAlternative): string {
  return alternative.kind === "execution_profile"
    ? "Execution profile"
    : alternative.parameters.length === 1
      ? "Stored secret"
      : "Stored secrets";
}

function parameterLabel(name: string): string {
  return props.action.parameters.find((parameter) => parameter.name === name)?.label ?? name;
}

function selectMode(mode: string): void {
  const next = Object.fromEntries(
    Object.entries(actionObject.value).filter(([name]) => name !== "execution_profile"),
  );
  const nextConfiguration = Object.fromEntries(
    Object.entries(configuration.value).filter(
      ([name]) => !allSecretParameters.value.includes(name),
    ),
  );

  next.configuration = nextConfiguration;
  emit("update:modelValue", next);
  selectedMode.value = mode;
}

function secretValue(parameter: string): string {
  const value = configuration.value[parameter];
  return typeof value === "string" ? value : "";
}

function missingSecret(parameter: string): boolean {
  const value = secretValue(parameter);
  return (
    Boolean(value) &&
    !secretOptions.value.some((secret) => secretRef(secret.scope, secret.name) === value)
  );
}

function setSecret(parameter: string, event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  const nextConfiguration = { ...configuration.value };

  if (value) {
    nextConfiguration[parameter] = value;
  } else {
    delete nextConfiguration[parameter];
  }

  emit("update:modelValue", {
    ...actionObject.value,
    configuration: nextConfiguration,
  });
}

function setProfile(id: string): void {
  const profile = profiles.profiles.find((candidate) => candidate.id === id);
  emit("update:modelValue", {
    ...actionObject.value,
    execution_profile: { id, name: profile?.name ?? "execution-profile" },
  });
}

onMounted(() => {
  if (!secrets.secrets.length) {
    void secrets.refreshSecrets();
  }

  if (!profiles.profiles.length) {
    void profiles.refresh();
  }
});
</script>
