<template>
  <section class="pane h-full overflow-hidden">
    <div class="panel h-full min-h-0">
      <PanelHeader
        title="Execution Profiles"
        icon="key"
        eyebrow="Private provider identities"
        description="Configure how a desktop agent collects login files and how workers expose an encrypted bundle to each bound action."
      >
        <button class="btn" :disabled="loading" @click="refresh"><Icon name="refresh" /> Refresh</button>
        <button class="btn btn-primary" @click="beginCreate"><Icon name="plus" /> New profile</button>
      </PanelHeader>

      <p v-if="error" class="mb-2 text-sm text-danger">{{ error }}</p>
      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <MetricCard label="Profiles" :value="filtered.length" />
        <MetricCard label="Ready" :value="profiles.filter((profile) => profile.health === 'ready').length" />
        <MetricCard label="Need attention" :value="profiles.filter((profile) => !['ready', 'disabled'].includes(profile.health)).length" />
      </div>

      <LoadingPanel v-if="loading && !profiles.length" compact message="Loading execution profiles…" />
      <EmptyState v-else-if="!filtered.length" compact icon="key" title="No execution profiles" description="Create a profile, approve it on a desktop agent, then bind an action with @profile(&quot;name&quot;)." />
      <div v-else class="table-scroll min-h-0 flex-1">
        <DataTable bare table-class="entity-banner-table table-resize-disabled">
          <thead><tr><th>Profile</th><th>Status</th><th>Publication</th><th class="entity-banner-actions"><span class="sr-only">Actions</span></th></tr></thead>
          <tbody>
            <tr v-for="profile in filtered" :key="profile.id">
              <td>
                <div class="entity-banner-content">
                  <span class="entity-banner-title">{{ profile.name }}</span>
                  <span class="entity-banner-meta">{{ profile.credential_scopes.join(", ") }} · config v{{ profile.config_version }}</span>
                </div>
              </td>
              <td><span class="badge">{{ profile.health }}</span><span v-if="profile.last_error" class="ml-2 text-danger">{{ profile.last_error }}</span></td>
              <td>
                <div v-if="profile.current_revision" class="entity-banner-content">
                  <span>r{{ profile.current_revision }} · {{ formatDate(profile.published_at) }}</span>
                  <span class="entity-banner-meta">{{ profile.current_digest?.slice(0, 12) }} · publisher {{ profile.current_publisher_id?.slice(0, 8) ?? "system" }}</span>
                </div>
                <span v-else>Not published</span>
              </td>
              <td class="entity-banner-actions whitespace-nowrap">
                <button class="btn btn-sm" :disabled="!profile.enabled" @click="testProfile(profile)"><Icon name="check" :size="13" /> Test</button>
                <button class="btn btn-sm" :disabled="!profile.enabled" @click="rotate(profile)"><Icon name="refresh" :size="13" /> Rotate</button>
                <button class="btn btn-sm" @click="beginEdit(profile)"><Icon name="edit" :size="13" /> Edit</button>
                <button class="btn btn-sm btn-danger ml-1" @click="remove(profile)"><Icon name="trash" :size="13" /></button>
              </td>
            </tr>
          </tbody>
        </DataTable>
      </div>
    </div>

    <div v-if="editing" class="modal-backdrop" @click.self="editing = false">
      <div class="modal max-h-[90vh] w-[min(880px,94vw)] overflow-y-auto">
        <h2 class="text-lg font-semibold">{{ editingId ? "Edit execution profile" : "New execution profile" }}</h2>
        <p class="mt-1 text-sm text-fg-muted">Commands and source paths require separate approval on each desktop agent. Bundle contents are never shown here.</p>
        <div class="mt-3 flex flex-wrap gap-2">
          <button class="btn btn-sm" @click="applyTemplate('aws')">AWS SSO example</button>
          <button class="btn btn-sm" @click="applyTemplate('claude')">Claude example</button>
          <button class="btn btn-sm" @click="applyTemplate('github')">GitHub / Copilot example</button>
        </div>
        <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
          <label class="field"><span>Name</span><input v-model="draft.name" class="input" /></label>
          <label class="field"><span>Credential scopes (comma-separated)</span><input v-model="scopeText" class="input" placeholder="aws, cloud.production" /></label>
        </div>
        <label class="field mt-3"><span>Description</span><input v-model="draft.description" class="input" /></label>
        <label class="field mt-3"><span>Collection specification</span><textarea v-model="collectionText" class="input min-h-56 font-mono text-xs" spellcheck="false" /></label>
        <label class="field mt-3"><span>Exposure specification</span><textarea v-model="exposureText" class="input min-h-32 font-mono text-xs" spellcheck="false" /></label>
        <label class="mt-3 flex items-center gap-2 text-sm"><input v-model="draft.enabled" type="checkbox" /> Enabled</label>
        <p v-if="formError" class="mt-3 text-sm text-danger">{{ formError }}</p>
        <div class="mt-5 flex justify-end gap-2">
          <button class="btn" @click="editing = false">Cancel</button>
          <button class="btn btn-primary" :disabled="saving" @click="save"><Icon name="save" /> Save profile</button>
        </div>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import type { ExecutionProfile, ExecutionProfileInput } from "../../core/domain/models";
import { deleteExecutionProfile, fetchExecutionProfiles, putExecutionProfile, rotateExecutionProfile, testExecutionProfile } from "../../core/api/commandCenterApi";
import { formatDate } from "../../core/utils/format";
import { useAppStore } from "../adapters/pinia/app";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";

const app = useAppStore();
const profiles = ref<ExecutionProfile[]>([]);
const loading = ref(false);
const saving = ref(false);
const editing = ref(false);
const editingId = ref("");
const error = ref("");
const formError = ref("");
const scopeText = ref("");
const collectionText = ref("");
const exposureText = ref("");
const draft = reactive<ExecutionProfileInput>(emptyProfile());
const filtered = computed(() => !app.normalizedSearch ? profiles.value : profiles.value.filter((profile) => [profile.name, profile.description, ...profile.credential_scopes].some((value) => value.toLowerCase().includes(app.normalizedSearch))));

function emptyProfile(): ExecutionProfileInput {
  return { name: "", description: "", credential_scopes: [], collection: { version: 1, probe: null, refresh: null, sources: [] }, exposure: { version: 1, home_overlay: true, environment: {} }, enabled: true };
}

function reset(input: ExecutionProfileInput) {
  Object.assign(draft, structuredClone(input));
  scopeText.value = input.credential_scopes.join(", ");
  collectionText.value = JSON.stringify(input.collection, null, 2);
  exposureText.value = JSON.stringify(input.exposure, null, 2);
  formError.value = "";
}

function beginCreate() { editingId.value = ""; reset(emptyProfile()); editing.value = true; }

function beginEdit(profile: ExecutionProfile) { editingId.value = profile.id; reset(profile); editing.value = true; }

function applyTemplate(kind: "aws" | "claude" | "github") {
  const templates: Record<typeof kind, ExecutionProfileInput> = {
    aws: { name: "aws-production", description: "AWS IAM Identity Center login", credential_scopes: ["aws"], collection: { version: 1, probe: { argv: ["aws", "sts", "get-caller-identity", "--profile", "runinator", "--no-cli-pager"] }, refresh: { argv: ["aws", "sso", "login", "--profile", "runinator"], interactive: true }, sources: [{ type: "file", path: "~/.aws/config", target: ".aws/config" }, { type: "directory", path: "~/.aws/sso/cache", glob: "*.json", target: ".aws/sso/cache" }] }, exposure: { version: 1, home_overlay: true, environment: { AWS_PROFILE: "runinator", AWS_REGION: "us-east-1" } }, enabled: true },
    claude: { name: "claude-default", description: "Claude Code desktop login", credential_scopes: ["claude"], collection: { version: 1, probe: null, refresh: null, sources: [{ type: "command", command: { argv: ["keychain-export", "--service", "Claude Code-credentials", "--quiet"] }, target: ".claude/.credentials.json" }] }, exposure: { version: 1, home_overlay: true, environment: {} }, enabled: true },
    github: { name: "github-default", description: "GitHub CLI and Copilot login", credential_scopes: ["github", "copilot"], collection: { version: 1, probe: { argv: ["gh", "auth", "status"] }, refresh: { argv: ["gh", "auth", "login"], interactive: true }, sources: [{ type: "directory", path: "~/.config/gh", glob: "*", target: ".config/gh" }] }, exposure: { version: 1, home_overlay: true, environment: { GH_CONFIG_DIR: "${PROFILE_HOME}/.config/gh" } }, enabled: true },
  };
  reset(templates[kind]);
}

async function refresh() { loading.value = true; error.value = ""; try { profiles.value = await fetchExecutionProfiles(); } catch (reason) { error.value = String(reason); } finally { loading.value = false; } }

async function save() {
  formError.value = "";

  try {
    const collection = JSON.parse(collectionText.value) as ExecutionProfileInput["collection"];
    const exposure = JSON.parse(exposureText.value) as ExecutionProfileInput["exposure"];
    const profile: ExecutionProfileInput = { ...structuredClone(draft), credential_scopes: scopeText.value.split(",").map((value) => value.trim()).filter(Boolean), collection, exposure };
    if (!profile.name.trim()) {throw new Error("Name is required.");}
    saving.value = true;
    await putExecutionProfile(editingId.value || crypto.randomUUID(), profile);
    editing.value = false;
    await refresh();
  } catch (reason) { formError.value = String(reason); } finally { saving.value = false; }
}

async function remove(profile: ExecutionProfile) { if (!window.confirm(`Delete execution profile “${profile.name}”?`)) {return;} await deleteExecutionProfile(profile.id); await refresh(); }

async function rotate(profile: ExecutionProfile) { await rotateExecutionProfile(profile.id); await refresh(); }

async function testProfile(profile: ExecutionProfile) { await testExecutionProfile(profile.id); await refresh(); }

onMounted(refresh);
</script>
