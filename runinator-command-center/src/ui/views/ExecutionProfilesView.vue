<template>
  <section class="pane h-full overflow-hidden">
    <div class="panel h-full min-h-0">
      <PanelHeader
        title="Execution Profiles"
        icon="key"
        eyebrow="Private provider identities"
        description="Collect desktop login files into encrypted bundles and expose each bundle only to its bound action."
      >
        <button class="btn" :disabled="loading" @click="refresh">
          <Icon name="refresh" /> Refresh</button
        ><button class="btn btn-primary" :disabled="!canMutate" @click="beginCreate">
          <Icon name="plus" /> New profile
        </button>
      </PanelHeader>
      <p v-if="error" class="mb-2 text-sm text-danger" role="alert">{{ error }}</p>
      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <MetricCard label="Profiles" :value="filtered.length" /><MetricCard
          label="Ready"
          :value="profiles.filter((p) => p.health === 'ready').length"
        /><MetricCard
          label="Need attention"
          :value="profiles.filter((p) => !['ready', 'disabled'].includes(p.health)).length"
        />
      </div>
      <LoadingPanel
        v-if="loading && !profiles.length"
        compact
        message="Loading execution profiles…"
      />
      <EmptyState
        v-else-if="!filtered.length"
        compact
        icon="key"
        title="No execution profiles"
        description='Start with an editable template, approve it on a desktop agent, then bind an action with @profile("name").'
      />
      <div v-else class="table-scroll min-h-0 flex-1">
        <DataTable bare table-class="entity-banner-table table-resize-disabled"
          ><thead>
            <tr>
              <th>Profile</th>
              <th>Status</th>
              <th>Publication</th>
              <th />
            </tr>
          </thead>
          <tbody>
            <tr v-for="profile in filtered" :key="profile.id">
              <td>
                <div class="entity-banner-content">
                  <span class="entity-banner-title">{{ profile.name }}</span
                  ><span class="entity-banner-meta">{{
                    profile.description || "No description"
                  }}</span>
                  <div class="chips">
                    <span v-for="scope in profile.credential_scopes" :key="scope" class="chip">{{
                      scope
                    }}</span>
                  </div>
                </div>
              </td>
              <td>
                <span class="badge">{{ profile.health }}</span>
                <p v-if="profile.last_error" class="mt-1 text-xs text-danger">
                  {{ profile.last_error }}
                </p>
                <span class="entity-banner-meta">Config v{{ profile.config_version }}</span>
              </td>
              <td>
                <div v-if="profile.current_revision">
                  <span
                    >Revision {{ profile.current_revision }} ·
                    {{ formatDate(profile.published_at) }}</span
                  >
                  <div class="entity-banner-meta">{{ profile.current_digest?.slice(0, 12) }}</div>
                </div>
                <span v-else class="text-fg-muted">Waiting for desktop publication</span>
              </td>
              <td class="entity-banner-actions whitespace-nowrap">
                <button
                  class="btn btn-sm"
                  :disabled="!profile.enabled || !canMutate"
                  @click="testProfile(profile)"
                >
                  <Icon name="check" :size="13" /> Test</button
                ><button class="btn btn-sm" :disabled="!profile.enabled || !canMutate" @click="rotate(profile)">
                  <Icon name="refresh" :size="13" /> Rotate</button
                ><button class="btn btn-sm" :disabled="!canMutate" @click="beginEdit(profile)">
                  <Icon name="edit" :size="13" /> Edit</button
                ><button
                  class="btn btn-sm btn-danger"
                  :disabled="!canMutate"
                  :aria-label="`Delete ${profile.name}`"
                  @click="remove(profile)"
                >
                  <Icon name="trash" :size="13" />
                </button>
              </td>
            </tr></tbody
        ></DataTable>
      </div>
    </div>

    <Modal
      v-if="editing"
      :title="editingId ? 'Edit execution profile' : 'New execution profile'"
      description="Changes require fresh approval on each desktop agent. Collected contents remain private."
      width="min(940px, 96vw)"
      @close="closeEditor"
    >
      <div class="templates">
        <span>Start from</span
        ><button class="btn btn-sm" @click="applyTemplate('aws')">AWS SSO</button
        ><button class="btn btn-sm" @click="applyTemplate('claude')">Claude</button
        ><button class="btn btn-sm" @click="applyTemplate('github')">GitHub / Copilot</button>
      </div>
      <nav class="tabs">
        <button
          v-for="tab in tabs"
          :key="tab.id"
          :class="{ active: activeTab === tab.id }"
          @click="activeTab = tab.id"
        >
          {{ tab.label }} <b v-if="errorCount(tab.id)">{{ errorCount(tab.id) }}</b>
        </button>
      </nav>
      <div class="editor-body">
        <section v-if="activeTab === 'identity'" class="section">
          <header>
            <h3>Identity and access</h3>
            <p>Set the stable workflow name and provider capabilities this profile can satisfy.</p>
          </header>
          <div class="form-grid !grid-cols-1 sm:!grid-cols-2">
            <Field label="Profile name" path="name"
              ><input v-model="draft.name" class="input" placeholder="aws-production" /></Field
            ><Field label="Description" path="description"
              ><input
                v-model="draft.description"
                class="input"
                placeholder="Production AWS SSO login"
            /></Field>
          </div>
          <div class="field">
            <span>Credential scopes</span>
            <div class="token-input">
              <span v-for="(scope, i) in draft.credential_scopes" :key="scope" class="chip"
                >{{ scope }} <button @click="draft.credential_scopes.splice(i, 1)">×</button></span
              ><input
                v-model="scopeEntry"
                placeholder="Type a scope and press Enter"
                @keydown.enter.prevent="addScope"
                @keydown.,.prevent="addScope"
                @blur="addScope"
              />
            </div>
            <small v-if="fieldError('credential_scopes')" class="field-error">{{
              fieldError("credential_scopes")
            }}</small
            ><small v-else>Examples: aws, claude, github, copilot.</small>
          </div>
          <label class="toggle"
            ><input v-model="draft.enabled" type="checkbox" /><span
              ><strong>Profile enabled</strong
              ><small>Disabled profiles cannot publish or run.</small></span
            ></label
          >
        </section>
        <section v-else-if="activeTab === 'collection'" class="section">
          <header>
            <h3>Desktop collection</h3>
            <p>
              Commands execute directly as argv. Each change invalidates prior desktop approval.
            </p>
          </header>
          <div class="grid gap-3 md:grid-cols-2">
            <CommandArgvEditor
              :model-value="draft.collection.probe"
              optional
              label="Probe command"
              description="Non-interactive readiness check."
              :error="fieldError('probe')"
              @update:model-value="draft.collection.probe = $event"
            /><CommandArgvEditor
              :model-value="draft.collection.refresh"
              optional
              allow-interactive
              label="Refresh command"
              description="Runs on rotation or approaching expiry."
              :error="fieldError('refresh')"
              @update:model-value="draft.collection.refresh = $event"
            />
          </div>
          <div class="source-head">
            <div>
              <h3>Bundle sources</h3>
              <p>Destinations are relative to the private root.</p>
            </div>
            <div>
              <button class="btn btn-sm" @click="addSource('file')">+ File</button
              ><button class="btn btn-sm" @click="addSource('directory')">+ Folder</button
              ><button class="btn btn-sm" @click="addSource('command')">+ Command output</button>
            </div>
          </div>
          <small v-if="fieldError('sources')" class="field-error">{{
            fieldError("sources")
          }}</small>
          <article v-for="(source, i) in draft.collection.sources" :key="i" class="source">
            <header>
              <b>{{ i + 1 }}. {{ sourceLabel(source.type) }}</b
              ><button class="btn btn-sm" @click="draft.collection.sources.splice(i, 1)">
                <Icon name="trash" :size="13" />
              </button>
            </header>
            <div v-if="source.type !== 'command'" class="form-grid !grid-cols-1 sm:!grid-cols-2">
              <Field
                :label="source.type === 'file' ? 'Local file' : 'Local folder'"
                :path="`sources.${i}.path`"
                ><input
                  v-model="source.path"
                  class="input font-mono"
                  placeholder="~/.config/provider" /></Field
              ><Field
                v-if="source.type === 'directory'"
                label="File glob"
                :path="`sources.${i}.glob`"
                ><input v-model="source.glob" class="input font-mono" placeholder="**/*.json"
              /></Field>
            </div>
            <CommandArgvEditor
              v-else
              :model-value="source.command"
              label="Collector command"
              description="Standard output becomes the destination file."
              :error="fieldError(`sources.${i}.command`)"
              @update:model-value="updateSourceCommand(i, $event)"
            />
            <Field label="Bundle destination" :path="`sources.${i}.target`"
              ><input
                v-model="source.target"
                class="input font-mono"
                placeholder=".config/provider/session.json"
            /></Field>
          </article>
        </section>
        <section v-else class="section">
          <header>
            <h3>Worker exposure</h3>
            <p>Applied only to one provider execution—never to the worker process globally.</p>
          </header>
          <label class="toggle"
            ><input v-model="draft.exposure.home_overlay" type="checkbox" /><span
              ><strong>Use a private HOME overlay</strong
              ><small>The effect-private profile root becomes HOME.</small></span
            ></label
          >
          <div class="source-head">
            <div>
              <h3>Environment variables</h3>
              <p>Values may use ${PROFILE_ROOT} and ${PROFILE_HOME}.</p>
            </div>
            <button class="btn btn-sm" @click="addEnvironment">+ Variable</button>
          </div>
          <div v-for="(row, i) in environmentRows" :key="row.id" class="env-row">
            <Field label="Name" :error="environmentError(i, 'name')"
              ><input
                v-model="row.name"
                class="input font-mono"
                placeholder="GH_CONFIG_DIR" /></Field
            ><Field label="Value" :error="environmentError(i, 'value')"
              ><input
                v-model="row.value"
                class="input font-mono"
                placeholder="${PROFILE_HOME}/.config/gh" /></Field
            ><button class="btn btn-sm" @click="environmentRows.splice(i, 1)">
              <Icon name="trash" :size="13" />
            </button>
          </div>
          <div v-if="!environmentRows.length" class="empty">No environment overrides.</div>
        </section>
      </div>
      <p v-if="formError" class="text-sm text-danger">{{ formError }}</p>
      <template #actions
        ><div class="mr-auto text-sm" :class="validation.valid ? 'text-success' : 'text-warning'">
          {{ validation.summary }}
        </div>
        <button class="btn" @click="closeEditor">Cancel</button
        ><button class="btn btn-primary" :disabled="saving || !validation.valid || !canMutate" @click="save">
          <Icon name="save" /> {{ saving ? "Saving…" : "Save profile" }}
        </button></template
      >
    </Modal>
  </section>
</template>

<script setup lang="ts">
import { computed, defineComponent, h, onMounted, reactive, ref, toRaw } from "vue";
import type {
  ExecutionProfile,
  ExecutionProfileCommand,
  ExecutionProfileInput,
  ExecutionProfileSource,
} from "../../core/domain/models";
import { validateExecutionProfile } from "../../core/domain/models/execution-profile/validation";
import { formatDate } from "../../core/utils/format";
import { useAppStore } from "../adapters/pinia/app";
import { useExecutionProfilesStore } from "../adapters/pinia/executionProfiles";
import { storeToRefs } from "pinia";
import CommandArgvEditor from "../components/execution-profiles/CommandArgvEditor.vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import MetricCard from "../components/shared/MetricCard.vue";
import Modal from "../components/shared/Modal.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
type Tab = "identity" | "collection" | "exposure";
interface EnvRow {
  id: string;
  name: string;
  value: string;
}
const app = useAppStore(),
  profileStore = useExecutionProfilesStore(),
  loading = ref(false),
  saving = ref(false),
  editing = ref(false),
  editingId = ref(""),
  error = ref(""),
  formError = ref(""),
  scopeEntry = ref(""),
  activeTab = ref<Tab>("identity"),
  environmentRows = ref<EnvRow[]>([]);
const { profiles, filteredProfiles: filtered } = storeToRefs(profileStore);
const canMutate = computed(() => app.can("credentials:manage"));
const draft = reactive<ExecutionProfileInput>(emptyProfile());
const tabs: { id: Tab; label: string }[] = [
  { id: "identity", label: "1. Identity" },
  { id: "collection", label: "2. Collection" },
  { id: "exposure", label: "3. Exposure" },
];

function buildInput() {
  return {
    ...structuredClone(toRaw(draft)),
    name: draft.name.trim(),
    exposure: {
      ...structuredClone(toRaw(draft.exposure)),
      environment: Object.fromEntries(
        environmentRows.value.filter((r) => r.name.trim()).map((r) => [r.name.trim(), r.value]),
      ),
    },
  };
}

const validation = computed(() => {
  const result = validateExecutionProfile(buildInput()),
    names = environmentRows.value.map((r) => r.name.trim().toLowerCase());
  environmentRows.value.forEach((r, i) => {
    if (!r.name.trim()) {
      result.fields[`environment-row.${String(i)}.name`] = "Name is required.";
    } else if (names.indexOf(r.name.trim().toLowerCase()) !== i) {
      result.fields[`environment-row.${String(i)}.name`] = "Names must be unique.";
    }
  });
  const count = Object.keys(result.fields).length;
  return {
    fields: result.fields,
    valid: !count,
    summary: !count
      ? "Profile configuration is valid."
      : `${String(count)} field${count === 1 ? " needs" : "s need"} attention.`,
  };
});
const Field = defineComponent({
  props: {
    label: { type: String, required: true },
    path: { type: String, default: "" },
    error: { type: String, default: "" },
  },
  setup(props, { slots }) {
    return () => {
      const error = props.error ? props.error : props.path ? fieldError(props.path) : undefined;

      return h("label", { class: "field" }, [
        h("span", props.label),
        slots.default?.(),
        error ? h("small", { class: "field-error" }, error) : null,
      ]);
    };
  },
});

function emptyProfile(): ExecutionProfileInput {
  return {
    name: "",
    description: "",
    credential_scopes: [],
    collection: { version: 1, probe: null, refresh: null, sources: [] },
    exposure: { version: 1, home_overlay: true, environment: {} },
    enabled: true,
  };
}

function reset(input: ExecutionProfileInput) {
  Object.assign(draft, structuredClone(input));
  environmentRows.value = Object.entries(input.exposure.environment).map(([name, value]) => ({
    id: crypto.randomUUID(),
    name,
    value,
  }));
  scopeEntry.value = "";
  formError.value = "";
  activeTab.value = "identity";
}

function beginCreate() {
  editingId.value = "";
  reset(emptyProfile());
  editing.value = true;
}

function beginEdit(p: ExecutionProfile) {
  editingId.value = p.id;
  reset(p);
  editing.value = true;
}

function closeEditor() {
  if (!saving.value) {
    editing.value = false;
  }
}

function applyTemplate(kind: "aws" | "claude" | "github") {
  const t: Record<typeof kind, ExecutionProfileInput> = {
    aws: {
      name: "aws-production",
      description: "AWS IAM Identity Center login",
      credential_scopes: ["aws"],
      collection: {
        version: 1,
        probe: {
          argv: ["aws", "sts", "get-caller-identity", "--profile", "runinator", "--no-cli-pager"],
        },
        refresh: { argv: ["aws", "sso", "login", "--profile", "runinator"], interactive: true },
        sources: [
          { type: "file", path: "~/.aws/config", target: ".aws/config" },
          { type: "directory", path: "~/.aws/sso/cache", glob: "*.json", target: ".aws/sso/cache" },
        ],
      },
      exposure: {
        version: 1,
        home_overlay: true,
        environment: { AWS_PROFILE: "runinator", AWS_REGION: "us-east-1" },
      },
      enabled: true,
    },
    claude: {
      name: "claude-default",
      description: "Claude Code desktop login",
      credential_scopes: ["claude"],
      collection: {
        version: 1,
        sources: [
          {
            type: "command",
            command: {
              argv: ["keychain-export", "--service", "Claude Code-credentials", "--quiet"],
            },
            target: ".claude/.credentials.json",
          },
        ],
      },
      exposure: { version: 1, home_overlay: true, environment: {} },
      enabled: true,
    },
    github: {
      name: "github-default",
      description: "GitHub CLI and Copilot login",
      credential_scopes: ["github", "copilot"],
      collection: {
        version: 1,
        probe: { argv: ["gh", "auth", "status"] },
        refresh: { argv: ["gh", "auth", "login"], interactive: true },
        sources: [{ type: "directory", path: "~/.config/gh", glob: "*", target: ".config/gh" }],
      },
      exposure: {
        version: 1,
        home_overlay: true,
        environment: { GH_CONFIG_DIR: "${PROFILE_HOME}/.config/gh" },
      },
      enabled: true,
    },
  };
  reset(t[kind]);
}

function addScope() {
  for (const value of scopeEntry.value
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean)) {
    if (!draft.credential_scopes.some((s) => s.toLowerCase() === value.toLowerCase())) {
      draft.credential_scopes.push(value);
    }
  }

  scopeEntry.value = "";
}

function addSource(type: ExecutionProfileSource["type"]) {
  draft.collection.sources.push(
    type === "file"
      ? { type, path: "", target: "" }
      : type === "directory"
        ? { type, path: "", glob: "*", target: "" }
        : { type, command: { argv: [""] }, target: "" },
  );
}

function updateSourceCommand(i: number, command: ExecutionProfileCommand | null) {
  const source = draft.collection.sources[i];

  if (source.type === "command") {
    source.command = command ?? { argv: [""] };
  }
}

function addEnvironment() {
  environmentRows.value.push({ id: crypto.randomUUID(), name: "", value: "" });
}

function fieldError(path: string) {
  return validation.value.fields[path];
}

function environmentError(i: number, field: "name" | "value") {
  const row = environmentRows.value[i];
  const rowError = fieldError(`environment-row.${String(i)}.${field}`);

  return rowError ? rowError : row.name ? fieldError(`environment.${row.name}.${field}`) : undefined;
}

function errorCount(tab: Tab) {
  const prefixes = {
    identity: ["name", "description", "credential_scopes"],
    collection: ["collection", "probe", "refresh", "sources"],
    exposure: ["exposure", "environment"],
  };
  return Object.keys(validation.value.fields).filter((path) =>
    prefixes[tab].some((p) => path.startsWith(p)),
  ).length;
}

function sourceLabel(type: ExecutionProfileSource["type"]) {
  return { file: "File", directory: "Folder", command: "Command output" }[type];
}

async function refresh() {
  loading.value = true;
  error.value = "";

  try {
    await profileStore.refresh();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
}

async function save() {
  if (!validation.value.valid) {
    return;
  }

  saving.value = true;
  formError.value = "";

  try {
    await profileStore.save(editingId.value ? editingId.value : crypto.randomUUID(), buildInput());
    editing.value = false;
  } catch (reason) {
    formError.value = String(reason);
  } finally {
    saving.value = false;
  }
}

async function remove(p: ExecutionProfile) {
  if (
    window.confirm(
      `Delete execution profile “${p.name}”? Its encrypted revisions will also be removed.`,
    )
  ) {
    await profileStore.remove(p.id);
  }
}

async function rotate(p: ExecutionProfile) {
  await profileStore.rotate(p.id);
}

async function testProfile(p: ExecutionProfile) {
  await profileStore.test(p.id);
}

onMounted(refresh);
</script>
<style scoped>
.chips {
  display: flex;
  gap: 0.25rem;
  margin-top: 0.25rem;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 999px;
  padding: 0.12rem 0.45rem;
  font-size: 0.72rem;
}
.templates,
.source-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  flex-wrap: wrap;
}
.templates {
  justify-content: flex-start;
  background: var(--color-surface-subtle);
  padding: 0.6rem;
  border-radius: 0.5rem;
}
.tabs {
  display: flex;
  border-bottom: 1px solid var(--color-border);
}
.tabs button {
  padding: 0.65rem 0.9rem;
  color: var(--color-fg-muted);
  border-bottom: 2px solid transparent;
}
.tabs button.active {
  color: var(--color-fg);
  border-color: var(--color-accent);
}
.tabs b {
  margin-left: 0.25rem;
  color: var(--color-danger);
}
.editor-body {
  max-height: 58vh;
  overflow: auto;
}
.section {
  display: grid;
  gap: 1rem;
}
.section header p,
.source-head p {
  font-size: 0.78rem;
  color: var(--color-fg-muted);
}
.token-input {
  display: flex;
  min-height: 2.5rem;
  align-items: center;
  gap: 0.4rem;
  flex-wrap: wrap;
  border: 1px solid var(--color-border);
  border-radius: 0.375rem;
  padding: 0.35rem;
}
.token-input input {
  min-width: 12rem;
  flex: 1;
  background: transparent;
  outline: 0;
}
.toggle {
  display: flex !important;
  gap: 0.65rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.5rem;
  padding: 0.75rem;
}
.toggle span {
  display: grid;
}
.toggle small {
  color: var(--color-fg-muted);
}
.source {
  display: grid;
  gap: 0.75rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.5rem;
  padding: 0.75rem;
  background: var(--color-surface-subtle);
}
.source > header {
  display: flex;
  justify-content: space-between;
}
.env-row {
  display: grid;
  grid-template-columns: 0.7fr 1.3fr auto;
  align-items: start;
  gap: 0.5rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.5rem;
  padding: 0.6rem;
}
.env-row > button {
  margin-top: 1.3rem;
}
.empty {
  padding: 1rem;
  text-align: center;
  border: 1px dashed var(--color-border);
  border-radius: 0.5rem;
  color: var(--color-fg-muted);
}
</style>
