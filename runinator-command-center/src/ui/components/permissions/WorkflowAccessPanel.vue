<template>
  <div class="grid min-h-0 gap-3 grid-rows-[minmax(220px,0.7fr)_minmax(220px,1fr)] overflow-hidden">
    <section class="grid min-h-0 min-w-0 content-start gap-3 overflow-hidden">
      <div class="panel-toolbar">
        <div>
          <div class="flex items-center gap-1">
            <h3 class="m-0 text-sm font-semibold text-fg">Resources</h3>
            <HelpBubble
              text="Select an independently managed resource to inspect or change its user and team grants."
              label="About resource access"
            />
          </div>
          <p class="m-0 text-xs text-fg-muted">{{ filteredResources.length }} shown</p>
        </div>
        <select :value="permissions.selectedResourceType" @change="changeResourceType">
          <option v-for="type in resourceTypes" :key="type.id" :value="type.id">{{ type.label }}</option>
        </select>
        <button
          class="btn btn-primary"
          type="button"
          :disabled="!permissions.selectedResourceId"
          @click="openModal"
        >
          <Icon name="plus" /><span>Add Access</span>
        </button>
      </div>
      <DataTable>
        <thead>
          <tr>
            <th>Name</th>
            <th>Type</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="resource in filteredResources"
            :key="resource.id"
            class="cursor-pointer"
            :class="{ selected: permissions.selectedResourceId === resource.id }"
            @click="permissions.selectResource(resource.id)"
          >
            <td>{{ resource.label }}</td>
            <td>{{ resourceTypeLabel }}</td>
          </tr>
        </tbody>
      </DataTable>
    </section>

    <section class="grid min-h-0 min-w-0 content-start gap-3 overflow-hidden">
      <div class="panel-toolbar">
        <div>
          <div class="flex items-center gap-1">
            <h3 class="m-0 text-sm font-semibold text-fg">Access</h3>
            <HelpBubble
              text="View the effective owner and manage grants for the selected resource."
              label="About access grants"
            />
          </div>
          <p class="m-0 text-xs text-fg-muted">Owner: {{ ownerLabel }}</p>
        </div>
        <div class="flex items-center gap-2">
          <select v-model="ownerChoice" :disabled="!permissions.selectedResourceId">
            <option value="">Transfer owner…</option>
            <option v-if="canTransferToPlatform" value="platform:">Platform</option>
            <option v-if="orgs.activeOrgId" :value="`organization:${orgs.activeOrgId}`">{{ orgs.activeOrg?.name ?? "Active organization" }}</option>
            <option v-for="team in permissions.teams" :key="`team:${team.id}`" :value="`team:${team.id}`">Team: {{ team.name }}</option>
            <option v-for="user in permissions.users" :key="`user:${user.id}`" :value="`user:${user.id}`">User: {{ user.username }}</option>
          </select>
          <button class="btn" type="button" :disabled="!ownerChoice" @click="transferOwner">Transfer</button>
        </div>
        <button
          class="btn"
          type="button"
          :disabled="!permissions.selectedResourceId"
          @click="permissions.refreshResourceGrants"
        >
          <Icon name="refresh" /><span>Refresh</span>
        </button>
      </div>
      <LoadingPanel
        v-if="loading && !permissions.resourceGrants.length"
        compact
        :message="loadingMessage || 'Loading resource access…'"
      />
      <DataTable v-else>
        <thead>
          <tr>
            <th>Principal</th>
            <th>Type</th>
            <th>Permission</th>
            <th>Created</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="grant in permissions.resourceGrants" :key="String(grant.id)">
            <td>{{ principalLabel(grant.principal_type, grant.principal_id) }}</td>
            <td>{{ grant.principal_type }}</td>
            <td>{{ grant.permission }}</td>
            <td>{{ formatDate(grant.created_at) }}</td>
            <td>
              <button
                class="btn btn-sm btn-ghost"
                type="button"
                @click="
                  revokeGrant(grant.id, principalLabel(grant.principal_type, grant.principal_id))
                "
              >
                Revoke
              </button>
            </td>
          </tr>
        </tbody>
      </DataTable>
    </section>

    <div v-if="modalOpen" class="modal-backdrop" @click.self="modalOpen = false">
      <form class="modal w-full max-w-[860px]" @submit.prevent="save">
        <header class="modal-header">
          <div class="flex items-center gap-1">
            <h2>Add Resource Access</h2>
            <HelpBubble
              text="Grant one user or team permission to view, edit, own, or use the selected resource."
              label="About adding resource access"
            />
          </div>
          <button class="btn btn-ghost" type="button" @click="modalOpen = false">
            <Icon name="x" />
          </button>
        </header>
        <div class="form-grid !grid-cols-1">
          <label
            ><span>Principal Type</span
            ><select
              v-model="permissions.grantDraft.principal_type"
              @change="permissions.grantDraft.principal_id = ''"
            >
              <option value="user">User</option>
              <option value="team">Team</option>
            </select></label
          >
          <label>
            <span>Principal</span>
            <select
              v-model="permissions.grantDraft.principal_id"
              required
              :disabled="principalOptions.length === 0"
            >
              <option value="">Principal</option>
              <option
                v-for="principal in principalOptions"
                :key="principal.id"
                :value="principal.id"
              >
                {{ principal.label }}
              </option>
            </select>
          </label>
          <label
            ><span>Permission</span
            ><select v-model="permissions.grantDraft.permission">
              <option v-for="level in permissionLevels" :key="level" :value="level">
                {{ permissionLabel(level) }}
              </option>
            </select></label
          >
        </div>
        <div class="modal-actions">
          <button class="btn" type="button" @click="modalOpen = false">Cancel</button>
          <button
            class="btn btn-primary"
            type="submit"
            :disabled="!permissions.selectedResourceId || !permissions.grantDraft.principal_id"
          >
            <Icon name="save" /><span>Save Access</span>
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { PrincipalType } from "../../../core/domain/models";
import type { AccessResourceType } from "../../../core/services/permissions";
import type { JsonRecord } from "../../../core/domain/models";
import { fetchResourceOwner, transferResourceOwner } from "../../../core/api/commandCenterApi";
import { formatDate } from "../../../core/utils/format";
import { useAppStore } from "../../adapters/pinia/app";
import { useActionsStore } from "../../adapters/pinia/actions";
import { useOrgsStore } from "../../adapters/pinia/orgs";
import { permissionLevels, usePermissionsStore } from "../../adapters/pinia/permissions";
import { useOperationLoading } from "../../composables/useOperationLoading";
import DataTable from "../shared/DataTable.vue";
import Icon from "../shared/Icon.vue";
import HelpBubble from "../shared/HelpBubble.vue";
import LoadingPanel from "../shared/LoadingPanel.vue";

const app = useAppStore();
const actions = useActionsStore();
const orgs = useOrgsStore();
const permissions = usePermissionsStore();
const { isLoading: loading, loadingMessage } = useOperationLoading(["Loading resource access"]);
const modalOpen = ref(false);
const owner = ref<JsonRecord | null>(null);
const ownerChoice = ref("");
const canTransferToPlatform = computed(() =>
  orgs.activeOrgId == null &&
  actions.has("roles:manage") &&
  ["setting", "execution_profile", "notification_policy"].includes(permissions.selectedResourceType),
);

const resourceTypes: { id: AccessResourceType; label: string }[] = [
  { id: "workflow", label: "Workflows" },
  { id: "pipeline", label: "Pipelines" },
  { id: "function_package", label: "Function packages" },
  { id: "console_session", label: "Console sessions" },
  { id: "setting", label: "Settings and secrets" },
  { id: "execution_profile", label: "Execution profiles" },
  { id: "orchestration_adapter", label: "Adapters" },
  { id: "library_file", label: "Library files" },
  { id: "notification_policy", label: "Standalone notification policies" },
];
const resourceTypeLabel = computed(() => resourceTypes.find((item) => item.id === permissions.selectedResourceType)?.label ?? "Resource");
const ownerLabel = computed(() => {
  const scope = owner.value?.owner as JsonRecord | undefined;

  if (!scope) {
    return permissions.selectedResourceId ? "Unavailable" : "Select a resource";
  }

  const kind = typeof scope.kind === "string" ? scope.kind : "unknown";
  const id = typeof scope.id === "string" ? scope.id : "";

  if (kind === "organization") {
    return orgs.activeOrg?.name ?? "Active organization";
  }

  if (kind === "team") {
    return permissions.teams.find((team) => team.id === id)?.name ?? id;
  }

  if (kind === "user") {
    return permissions.users.find((user) => user.id === id)?.username ?? id;
  }

  return "Platform";
});
const filteredResources = computed(() => {
  const list = permissions.accessResources;

  if (!app.normalizedSearch) {
    return list;
  }

  return list.filter((resource) =>
    [resource.id, resource.label]
      .filter(Boolean)
      .join(" ")
      .toLowerCase()
      .includes(app.normalizedSearch),
  );
});
const principalOptions = computed(() => {
  if (permissions.grantDraft.principal_type === "team") {
    return permissions.teams
      .filter((team) => team.id)
      .map((team) => ({ id: String(team.id), label: team.name }));
  }

  return permissions.users
    .filter((user) => user.id)
    .map((user) => ({ id: String(user.id), label: user.username }));
});

function selectFirstResource() {
  const first = filteredResources.value.at(0);

  if (first) {
    void permissions.selectResource(first.id);
  }
}

async function changeResourceType(event: Event) {
  await permissions.refreshAccessResources((event.target as HTMLSelectElement).value as AccessResourceType);
  selectFirstResource();
}

async function refreshOwner() {
  const resourceId = permissions.selectedResourceId;
  owner.value = resourceId
    ? await app.runOperation("Loading resource owner", () => fetchResourceOwner(permissions.selectedResourceType, resourceId))
    : null;
}

async function transferOwner() {
  const resourceId = permissions.selectedResourceId;

  if (!resourceId || !ownerChoice.value) {
    return;
  }

  const [scopeKind, scopeId] = ownerChoice.value.split(":", 2);
  owner.value = await app.runOperation("Transferring resource owner", () => transferResourceOwner(
    permissions.selectedResourceType,
    resourceId,
    scopeKind as "platform" | "organization" | "team" | "user",
    scopeKind === "platform" ? null : scopeId,
  ));
  ownerChoice.value = "";
}

function permissionLabel(level: string) {
  return level === "run" && ["setting", "execution_profile", "orchestration_adapter", "library_file"].includes(permissions.selectedResourceType)
    ? "Use"
    : level;
}

function openModal() {
  permissions.grantDraft.principal_id = "";
  modalOpen.value = true;
}

async function save() {
  await permissions.saveResourceGrant();

  if (!app.errorText) {
    modalOpen.value = false;
  }
}

async function revokeGrant(grantId: string | null, principal: string) {
  if (!grantId || !window.confirm(`Revoke resource access for ${principal}?`)) {
    return;
  }

  await permissions.revokeSelectedResourceGrant(grantId);
}

function principalLabel(type: PrincipalType, id: string) {
  if (type === "team") {
    return permissions.teams.find((team) => team.id === id)?.name ?? id;
  }

  return permissions.users.find((user) => user.id === id)?.username ?? id;
}

watch(
  filteredResources,
  () => {
    if (!permissions.selectedResourceId) {
      selectFirstResource();
    }
  },
  { immediate: true },
);

void permissions.refreshAccessResources();

watch(() => permissions.selectedResourceId, () => { void refreshOwner(); });
</script>
