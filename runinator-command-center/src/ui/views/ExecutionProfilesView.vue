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
          label="Published"
          :value="profiles.filter((p) => publicationHealth(p) === 'ready').length"
        /><MetricCard label="Need attention" :value="profiles.filter(needsAttention).length" />
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
        <DataTable
          bare
          table-class="entity-banner-table execution-profiles-table table-resize-disabled"
        >
          <thead>
            <tr>
              <th class="profile-name-column">Profile</th>
              <th class="profile-status-column">Desktop collection</th>
              <th class="profile-publication-column">Publication</th>
              <th class="entity-banner-actions profile-actions-column">
                <span class="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="profile in filtered" :key="profile.id">
              <td :title="profile.description || profile.name">
                <div class="entity-banner-content">
                  <span class="entity-banner-title">{{ profile.name }}</span
                  ><span class="entity-banner-meta">{{
                    profile.description || "No description"
                  }}</span>
                  <span v-if="isInheritedProfile(profile)" class="entity-banner-meta"
                    >Platform profile · switch to Platform scope to manage</span
                  >
                  <div class="chips">
                    <span v-for="scope in profile.credential_scopes" :key="scope" class="chip">{{
                      scope
                    }}</span>
                  </div>
                </div>
              </td>
              <td class="profile-status-cell">
                <div class="profile-collection-health">
                  <span
                    class="profile-health-dot"
                    :class="`is-${collectionHealth(profile).tone}`"
                    :title="collectionHealth(profile).label"
                  />
                  <div class="min-w-0">
                    <div class="profile-health-heading">
                      <strong>{{ collectionHealth(profile).label }}</strong>
                      <span>Config v{{ profile.config_version }}</span>
                    </div>
                    <p class="m-0 text-xs text-fg-muted">
                      {{ collectionHealth(profile).detail }}
                    </p>
                  </div>
                </div>
                <p v-if="collectionError(profile)" class="mt-1 text-xs text-danger">
                  {{ collectionError(profile) }}
                </p>
                <details v-if="collectionStatus(profile)" class="collection-status-details">
                  <summary>Desktop details</summary>
                  <div class="collection-status-grid">
                    <div>
                      <span>Approved desktops</span>
                      <strong
                        >{{ approvedAgentCount(profile) }} /
                        {{ collectionStatus(profile)?.agents.length }}</strong
                      >
                    </div>
                    <div>
                      <span>Last success</span>
                      <strong>{{ formatDate(lastSuccessAt(profile)) }}</strong>
                    </div>
                    <div>
                      <span>Latest operation</span>
                      <strong>{{ operationSummary(profile) }}</strong>
                    </div>
                  </div>
                  <div v-if="collectionStatus(profile)?.agents.length" class="collection-agents">
                    <div v-for="agent in collectionStatus(profile)?.agents" :key="agent.agent_id">
                      <strong>Desktop {{ shortAgentId(agent.agent_id) }}</strong>
                      <span>{{
                        agent.approval === "approved" ? "Approved" : "Approval required"
                      }}</span>
                      <span>Seen {{ formatDate(agent.last_seen_at) }}</span>
                    </div>
                  </div>
                </details>
              </td>
              <td class="profile-publication-cell">
                <div class="publication-card">
                  <span class="badge" :class="`publication-${publicationHealth(profile)}`">
                    {{ publicationLabel(profile) }}
                  </span>
                  <template v-if="publicationStatus(profile).current_revision">
                    <span class="publication-revision"
                      >Revision {{ publicationStatus(profile).current_revision }}</span
                    >
                    <span class="entity-banner-meta">
                      {{ formatDate(publicationStatus(profile).published_at) }}
                    </span>
                    <code v-if="profile.current_digest" class="publication-digest">
                      {{ profile.current_digest.slice(0, 12) }}
                    </code>
                  </template>
                  <span v-else class="text-fg-muted">No active desktop publication</span>
                </div>
              </td>
              <td class="entity-banner-actions profile-actions">
                <div class="profile-action-group">
                  <button
                    class="btn btn-sm profile-action"
                    type="button"
                    :disabled="!profile.enabled || !canManageProfile(profile)"
                    :title="profileActionTitle(profile, 'Dry run collection')"
                    aria-label="Dry run collection"
                    @click="testProfile(profile)"
                  >
                    <Icon name="check" :size="13" />
                  </button>
                  <button
                    class="btn btn-sm profile-action"
                    type="button"
                    :disabled="!profile.enabled || !canManageProfile(profile)"
                    :title="profileActionTitle(profile, 'Request rotation')"
                    aria-label="Request rotation"
                    @click="rotate(profile)"
                  >
                    <Icon name="refresh" :size="13" />
                  </button>
                  <button
                    class="btn btn-sm profile-action"
                    type="button"
                    :disabled="!canManageProfile(profile)"
                    :title="profileActionTitle(profile, 'Edit profile')"
                    aria-label="Edit profile"
                    @click="editProfile(profile)"
                  >
                    <Icon name="edit" :size="13" />
                  </button>
                  <button
                    class="btn btn-sm btn-danger profile-action"
                    type="button"
                    :disabled="!canManageProfile(profile)"
                    :aria-label="`Delete ${profile.name}`"
                    :title="profileActionTitle(profile, `Delete ${profile.name}`)"
                    @click="remove(profile)"
                  >
                    <Icon name="trash" :size="13" />
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </DataTable>
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
        ><button
          class="btn btn-primary"
          :disabled="saving || !validation.valid || !canMutate"
          @click="save"
        >
          <Icon name="save" /> {{ saving ? "Saving…" : "Save profile" }}
        </button></template
      >
    </Modal>
  </section>
</template>

<script setup lang="ts">
import {
  computed,
  defineComponent,
  h,
  onBeforeUnmount,
  onMounted,
  reactive,
  ref,
  toRaw,
  watch,
} from "vue";
import type {
  ExecutionProfile,
  ExecutionProfileCollectionStatus,
  ExecutionProfileCommand,
  ExecutionProfileInput,
  ExecutionProfileSource,
} from "../../core/domain/models";
import { validateExecutionProfile } from "../../core/domain/models/execution-profile/validation";
import { formatDate } from "../../core/utils/format";
import { useAppStore } from "../adapters/pinia/app";
import { useAuthStore } from "../adapters/pinia/auth";
import { useExecutionProfilesStore } from "../adapters/pinia/executionProfiles";
import { useOrgsStore } from "../adapters/pinia/orgs";
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
  auth = useAuthStore(),
  profileStore = useExecutionProfilesStore(),
  orgs = useOrgsStore(),
  loading = ref(false),
  saving = ref(false),
  editing = ref(false),
  editingId = ref(""),
  error = ref(""),
  formError = ref(""),
  scopeEntry = ref(""),
  activeTab = ref<Tab>("identity"),
  environmentRows = ref<EnvRow[]>([]);
const { profiles, filteredProfiles: filtered, collectionStatuses } = storeToRefs(profileStore);
const { activeOrgId } = storeToRefs(orgs);
const canMutate = computed(() => app.can("credentials:manage"));
const canSwitchToPlatform = computed(() => !auth.required || auth.user?.platform_role === "admin");
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
  const rawInput = toRaw(input);

  Object.assign(draft, structuredClone(rawInput));
  environmentRows.value = Object.entries(rawInput.exposure.environment).map(([name, value]) => ({
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

function isInheritedProfile(profile: ExecutionProfile) {
  return activeOrgId.value !== null && profile.org_id === null;
}

function canManageProfile(profile: ExecutionProfile) {
  return (
    canMutate.value &&
    (profile.org_id === activeOrgId.value ||
      (isInheritedProfile(profile) && canSwitchToPlatform.value))
  );
}

function profileActionHint(profile: ExecutionProfile) {
  if (!canMutate.value) {
    return "You do not have permission to manage execution profiles.";
  }

  if (isInheritedProfile(profile) && !canSwitchToPlatform.value) {
    return "Platform access is required to manage this profile.";
  }

  return isInheritedProfile(profile)
    ? "Switch to Platform scope to manage this profile."
    : undefined;
}

function profileActionTitle(profile: ExecutionProfile, action: string) {
  return profileActionHint(profile) ?? action;
}

type CollectionTone = "healthy" | "warning" | "error" | "muted";

function collectionStatus(profile: ExecutionProfile): ExecutionProfileCollectionStatus | undefined {
  return collectionStatuses.value[profile.id];
}

function publicationStatus(profile: ExecutionProfile) {
  const status = collectionStatus(profile);
  return {
    health: status?.publication_health ?? profile.health,
    current_revision: status?.current_revision ?? profile.current_revision,
    published_at: status?.published_at ?? profile.published_at,
    expires_at: status?.expires_at ?? profile.expires_at,
  };
}

function publicationHealth(profile: ExecutionProfile) {
  return publicationStatus(profile).health;
}

function publicationLabel(profile: ExecutionProfile) {
  return (
    {
      unpublished: "Unpublished",
      ready: "Published",
      expiring: "Expiring",
      expired: "Expired",
      disabled: "Disabled",
      testing: "Publishing",
      error: "Unavailable",
    } as const
  )[publicationHealth(profile)];
}

function collectionHealth(profile: ExecutionProfile): {
  tone: CollectionTone;
  label: string;
  detail: string;
} {
  if (!profile.enabled) {
    return {
      tone: "muted",
      label: "Desktop collection disabled",
      detail: "Enable the profile to collect.",
    };
  }

  const status = collectionStatus(profile);

  if (!status) {
    return {
      tone: "muted",
      label: "Loading desktop status",
      detail: "Checking approved desktop agents.",
    };
  }

  const operation = status.latest_operation;

  if (operation?.state === "failed") {
    return {
      tone: "error",
      label: `${operation.kind === "dry_run" ? "Dry run" : "Refresh"} failed`,
      detail: "The active publication was left unchanged.",
    };
  }

  if (operation?.state === "queued") {
    return {
      tone: "warning",
      label: `${operation.kind === "dry_run" ? "Dry run" : "Refresh"} queued`,
      detail: "Waiting for a desktop with local approval.",
    };
  }

  if (operation?.state === "running") {
    return {
      tone: "warning",
      label: `${operation.kind === "dry_run" ? "Dry run" : "Refresh"} running`,
      detail: "An approved desktop agent claimed the operation.",
    };
  }

  if (!status.agents.length) {
    return {
      tone: "muted",
      label: "Awaiting desktop agent",
      detail: "No desktop has reported this configuration.",
    };
  }

  const approved = approvedAgentCount(profile);

  if (!approved) {
    return {
      tone: "warning",
      label: "Local approval required",
      detail: "Approve this configuration on a desktop agent to collect it.",
    };
  }

  if (collectionError(profile)) {
    return {
      tone: "error",
      label: "Desktop collection needs attention",
      detail: "See the latest sanitized desktop error below.",
    };
  }

  if (lastSuccessAt(profile)) {
    return {
      tone: "healthy",
      label: "Desktop collection healthy",
      detail: `${String(approved)} approved desktop${approved === 1 ? "" : "s"}.`,
    };
  }

  return {
    tone: "warning",
    label: "Awaiting first collection",
    detail: `${String(approved)} approved desktop${approved === 1 ? "" : "s"} can collect it.`,
  };
}

function approvedAgentCount(profile: ExecutionProfile) {
  return (
    collectionStatus(profile)?.agents.filter((agent) => agent.approval === "approved").length ?? 0
  );
}

function lastSuccessAt(profile: ExecutionProfile) {
  return (
    collectionStatus(profile)?.agents.reduce<string | null>((latest, agent) => {
      if (!agent.last_success_at || (latest && latest >= agent.last_success_at)) {
        return latest;
      }

      return agent.last_success_at;
    }, null) ?? null
  );
}

function collectionError(profile: ExecutionProfile) {
  return (
    collectionStatus(profile)?.latest_operation?.error ??
    collectionStatus(profile)?.agents.find((agent) => agent.last_error)?.last_error ??
    profile.last_error
  );
}

function operationSummary(profile: ExecutionProfile) {
  const operation = collectionStatus(profile)?.latest_operation;

  if (!operation) {
    return "No requested operation";
  }

  const label = operation.kind === "dry_run" ? "Dry run" : "Refresh";
  return `${label} · ${operation.state}`;
}

function shortAgentId(agentId: string) {
  return agentId.slice(0, 8);
}

function needsAttention(profile: ExecutionProfile) {
  return (
    !["ready", "disabled"].includes(publicationHealth(profile)) ||
    ["warning", "error"].includes(collectionHealth(profile).tone)
  );
}

async function ensureProfileScope(profile: ExecutionProfile): Promise<boolean> {
  if (isInheritedProfile(profile)) {
    if (!(await orgs.setActivePlatform())) {
      error.value = "Could not switch to Platform scope to manage this execution profile.";
      return false;
    }
  }

  return true;
}

async function editProfile(profile: ExecutionProfile) {
  if (!(await ensureProfileScope(profile))) {
    return;
  }

  beginEdit(profile);
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
            type: "file",
            path: "~/.claude/CLAUDE.md",
            target: ".claude/CLAUDE.md",
          },
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

  return rowError
    ? rowError
    : row.name
      ? fieldError(`environment.${row.name}.${field}`)
      : undefined;
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
  addScope();

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
    await runProfileAction(p, () => profileStore.remove(p.id));
  }
}

async function rotate(p: ExecutionProfile) {
  await runProfileAction(p, () => profileStore.rotate(p.id));
}

async function testProfile(p: ExecutionProfile) {
  await runProfileAction(p, () => profileStore.test(p.id));
}

async function runProfileAction(profile: ExecutionProfile, action: () => Promise<void>) {
  if (!(await ensureProfileScope(profile))) {
    return;
  }

  error.value = "";

  try {
    await action();
  } catch (reason) {
    error.value = String(reason);
  }
}

let collectionStatusTimer: ReturnType<typeof window.setInterval> | undefined;

onMounted(() => {
  void refresh();
  collectionStatusTimer = window.setInterval(() => {
    void profileStore.refreshCollectionStatus().catch(() => undefined);
  }, 10_000);
});
onBeforeUnmount(() => {
  if (collectionStatusTimer) {
    window.clearInterval(collectionStatusTimer);
  }
});
watch(activeOrgId, () => void refresh());
</script>
<style scoped>
.chips {
  display: flex;
  gap: 0.25rem;
  flex-wrap: wrap;
  margin-top: 0.25rem;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 999px;
  padding: 0.12rem 0.45rem;
  background: var(--color-surface-subtle);
  font-size: 0.72rem;
}
:deep(.execution-profiles-table) {
  min-width: 54rem;
}
:deep(.execution-profiles-table .profile-name-column) {
  width: 34%;
}
:deep(.execution-profiles-table .profile-status-column) {
  width: 35%;
}
:deep(.execution-profiles-table .profile-publication-column) {
  width: 18%;
}
:deep(.execution-profiles-table .profile-actions-column),
:deep(.execution-profiles-table .profile-actions) {
  width: 8.5rem;
}
.profile-collection-health {
  display: flex;
  align-items: flex-start;
  gap: 0.55rem;
}
.profile-health-dot {
  width: 0.5rem;
  height: 0.5rem;
  flex: 0 0 auto;
  margin-top: 0.3rem;
  border-radius: 50%;
}
.profile-health-heading {
  display: flex;
  align-items: baseline;
  gap: 0.45rem;
}
.profile-health-heading span {
  color: var(--color-fg-muted);
  font-size: 0.7rem;
}
.profile-health-dot.is-healthy {
  background: var(--color-success-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-success-fg) 14%, transparent);
}
.profile-health-dot.is-warning {
  background: var(--color-warning-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-warning-fg) 14%, transparent);
}
.profile-health-dot.is-error {
  background: var(--color-danger-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-danger-fg) 14%, transparent);
}
.profile-health-dot.is-muted {
  background: var(--color-fg-muted);
}
.collection-status-details {
  margin-top: 0.45rem;
  color: var(--color-fg-muted);
  font-size: 0.72rem;
}
.collection-status-details summary {
  width: fit-content;
  cursor: pointer;
}
.collection-status-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(7rem, 1fr));
  gap: 0.4rem;
  margin-top: 0.45rem;
}
.collection-status-grid > div,
.collection-agents > div {
  display: grid;
  gap: 0.1rem;
}
.collection-status-grid > div {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.4rem;
  padding: 0.4rem;
  background: var(--color-surface-subtle);
}
.collection-status-grid span,
.collection-agents span {
  color: var(--color-fg-muted);
}
.collection-status-grid strong,
.collection-agents strong {
  color: var(--color-fg);
  font-weight: 600;
}
.collection-agents {
  display: grid;
  gap: 0.35rem;
  margin-top: 0.5rem;
  border-top: 1px solid var(--color-border-subtle);
  padding-top: 0.5rem;
}
.collection-agents > div {
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.4rem;
  padding: 0.4rem;
  background: var(--color-surface-subtle);
}
.publication-card {
  display: grid;
  align-content: start;
  justify-items: start;
  gap: 0.25rem;
}
.publication-revision {
  color: var(--color-fg);
  font-size: 0.78rem;
  font-weight: 600;
}
.publication-digest {
  max-width: 100%;
  overflow: hidden;
  color: var(--color-fg-muted);
  font-size: 0.68rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.profile-action-group {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0.25rem;
}
.profile-action {
  display: inline-flex;
  min-width: 0;
  justify-content: center;
  padding: 0.35rem;
}
.publication-ready {
  color: var(--color-success-fg);
}
.publication-expiring,
.publication-unpublished,
.publication-testing {
  color: var(--color-warning-fg);
}
.publication-expired,
.publication-error {
  color: var(--color-danger-fg);
}
@media (max-width: 48rem) {
  .collection-status-grid {
    grid-template-columns: 1fr;
  }
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
