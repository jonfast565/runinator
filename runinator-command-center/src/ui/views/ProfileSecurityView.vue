<template>
  <section class="pane grid min-h-0 content-start gap-4 overflow-auto p-4">
    <nav class="flex gap-1 border-b border-border-subtle" aria-label="Profile sections">
      <button
        v-for="item in tabs"
        :key="item.id"
        type="button"
        class="btn btn-ghost rounded-b-none border-transparent"
        :class="activeTab === item.id ? 'border-b-accent text-fg' : 'text-fg-muted'"
        @click="activeTab = item.id"
      >
        {{ item.label }}
      </button>
    </nav>

    <div v-if="activeTab === 'account'" class="grid max-w-[760px] gap-4">
      <form class="panel grid gap-4 p-4" @submit.prevent="saveProfile">
        <div>
          <h2 class="m-0 text-base font-semibold text-fg">Account</h2>
          <p class="mt-1 mb-0 text-xs text-fg-muted">
            Your username and access are managed by an administrator.
          </p>
        </div>
        <div class="form-grid !grid-cols-1">
          <label><span>Username</span><input :value="username" disabled /></label>
          <label
            ><span>Email</span
            ><input v-model.trim="email" type="email" maxlength="254" autocomplete="email"
          /></label>
        </div>
        <div class="modal-actions !mt-0">
          <button class="btn btn-primary" type="submit"><Icon name="save" />Save profile</button>
        </div>
      </form>

      <form class="panel grid gap-4 p-4" @submit.prevent="savePassword">
        <div>
          <h2 class="m-0 text-base font-semibold text-fg">Change password</h2>
          <p class="mt-1 mb-0 text-xs text-fg-muted">
            Changing your password signs out every other session and keeps this one active.
          </p>
        </div>
        <div class="form-grid !grid-cols-1">
          <label
            ><span>Current password</span
            ><input
              v-model="currentPassword"
              type="password"
              required
              autocomplete="current-password"
          /></label>
          <label
            ><span>New password</span
            ><input
              v-model="newPassword"
              type="password"
              required
              minlength="8"
              maxlength="256"
              autocomplete="new-password"
          /></label>
          <label
            ><span>Confirm new password</span
            ><input
              v-model="confirmPassword"
              type="password"
              required
              minlength="8"
              maxlength="256"
              autocomplete="new-password"
              :aria-invalid="Boolean(passwordError)"
            />
            <small v-if="passwordError" class="field-error" role="alert">{{ passwordError }}</small>
          </label>
        </div>
        <div class="modal-actions !mt-0">
          <button class="btn btn-primary" type="submit"><Icon name="lock" />Change password</button>
        </div>
      </form>
    </div>

    <div v-else-if="activeTab === 'sessions'" class="grid gap-3">
      <div class="panel-toolbar">
        <div>
          <h2 class="m-0 text-base font-semibold text-fg">Signed-in sessions</h2>
          <p class="mt-1 mb-0 text-xs text-fg-muted">
            Activity is recorded at five-minute granularity. IP addresses are direct peer addresses.
          </p>
        </div>
        <button class="btn btn-danger" type="button" @click="signOutOthers">
          <Icon name="lock" />Sign out other sessions
        </button>
      </div>
      <DataTable>
        <thead>
          <tr>
            <th>Client</th>
            <th>IP address</th>
            <th>Signed in</th>
            <th>Last active</th>
            <th>Expires</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="session in profile.sessions" :key="session.id">
            <td>
              <div class="flex items-center gap-2">
                <span>{{ clientLabel(session.user_agent) }}</span>
                <span
                  v-if="session.current"
                  class="rounded-pill bg-accent-soft px-2 py-0.5 text-[10px] font-semibold text-accent-text"
                  >Current</span
                >
              </div>
              <div
                v-if="session.user_agent"
                class="max-w-[360px] truncate text-[10px] text-fg-faint"
                :title="session.user_agent"
              >
                {{ session.user_agent }}
              </div>
            </td>
            <td>
              <code>{{ session.ip_address || "Unknown" }}</code>
            </td>
            <td>{{ formatDate(session.created_at) }}</td>
            <td>{{ formatDate(session.last_seen_at) }}</td>
            <td>{{ formatDate(session.expires_at) }}</td>
            <td>
              <button class="btn btn-sm" type="button" @click="signOutSession(session)">
                Sign out
              </button>
            </td>
          </tr>
          <tr v-if="!profile.sessions.length">
            <td colspan="6" class="text-center text-fg-muted">No active sessions.</td>
          </tr>
        </tbody>
      </DataTable>
    </div>

    <div v-else class="grid gap-3">
      <div class="panel-toolbar">
        <div>
          <h2 class="m-0 text-base font-semibold text-fg">Personal API keys</h2>
          <p class="mt-1 mb-0 text-xs text-fg-muted">
            Keys can never exceed your access in their selected scope.
          </p>
        </div>
        <button class="btn btn-primary" type="button" @click="openCreateKey">
          <Icon name="plus" />New key
        </button>
      </div>
      <div
        v-if="profile.revealedApiKey"
        class="flex items-start justify-between gap-3 rounded-md border border-accent bg-accent-soft p-3"
      >
        <div class="grid min-w-0 gap-1">
          <strong class="text-xs">Copy this secret now. It will not be shown again.</strong>
          <code class="break-words">{{ profile.revealedApiKey.secret }}</code>
        </div>
        <div class="btn-row">
          <button class="btn btn-sm" type="button" @click="copySecret">
            <Icon name="key" />Copy
          </button>
          <button class="btn btn-sm" type="button" @click="profile.clearRevealedKey">
            <Icon name="x" />Dismiss
          </button>
        </div>
      </div>
      <DataTable>
        <thead>
          <tr>
            <th>Name</th>
            <th>Scope</th>
            <th>Prefix</th>
            <th>Actions</th>
            <th>Status</th>
            <th>Last used</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="key in profile.apiKeys" :key="String(key.id)" :class="{ muted: key.disabled }">
            <td>{{ key.name }}</td>
            <td>{{ scopeName(key.org_id) }}</td>
            <td>
              <code>{{ key.key_prefix }}</code>
            </td>
            <td>{{ key.action_ceiling.join(", ") }}</td>
            <td>{{ key.disabled ? "revoked" : "active" }}</td>
            <td>{{ key.last_used_at ? formatDate(key.last_used_at) : "Never" }}</td>
            <td>
              <button class="btn btn-sm" type="button" @click="openEditKey(key)">Manage</button>
            </td>
          </tr>
          <tr v-if="!profile.apiKeys.length">
            <td colspan="7" class="text-center text-fg-muted">No personal API keys.</td>
          </tr>
        </tbody>
      </DataTable>
    </div>

    <div v-if="keyModalOpen" class="modal-backdrop" @click.self="closeKeyModal">
      <form class="modal w-full max-w-[720px]" @submit.prevent="saveKey">
        <header class="modal-header">
          <h2>{{ selectedKey ? "Manage API key" : "Create API key" }}</h2>
          <button class="btn btn-ghost" type="button" @click="closeKeyModal">
            <Icon name="x" />
          </button>
        </header>
        <div class="form-grid !grid-cols-1">
          <label><span>Name</span><input v-model.trim="keyName" required maxlength="100" /></label>
          <label v-if="!selectedKey"
            ><span>Scope</span
            ><select v-model="keyScope" @change="resetKeyActions">
              <option
                v-for="scope in profile.keyScopes"
                :key="scope.org_id || 'platform'"
                :value="scope.org_id || ''"
              >
                {{ scope.name }}
              </option>
            </select></label
          >
          <label><span>Expires at</span><input v-model="keyExpiry" type="datetime-local" /></label>
          <fieldset v-if="!selectedKey" class="grid gap-2 rounded border border-border-subtle p-3">
            <legend class="px-1 text-xs font-semibold text-fg">Allowed actions</legend>
            <label
              v-for="action in selectedScopeActions"
              :key="action"
              class="inline-flex items-center gap-2 text-xs"
              ><input v-model="keyActions" type="checkbox" :value="action" />{{ action }}</label
            >
          </fieldset>
          <label v-if="selectedKey" class="inline-flex items-center gap-2"
            ><input v-model="keyDisabled" type="checkbox" />Disabled</label
          >
        </div>
        <div class="modal-actions">
          <button
            v-if="selectedKey"
            class="btn btn-danger"
            type="button"
            @click="revokeSelectedKey"
          >
            Revoke
          </button>
          <button
            v-if="selectedKey && !selectedKey.disabled"
            class="btn"
            type="button"
            @click="rotateSelectedKey"
          >
            Rotate
          </button>
          <button class="btn" type="button" @click="closeKeyModal">Cancel</button>
          <button class="btn btn-primary" type="submit">
            <Icon name="save" />{{ selectedKey ? "Save" : "Create" }}
          </button>
        </div>
      </form>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import type { Action, ApiKey, AuthSessionSummary } from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";
import { useAppStore } from "../adapters/pinia/app";
import { useAuthStore } from "../adapters/pinia/auth";
import { useProfileSecurityStore } from "../adapters/pinia/profileSecurity";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";

type Tab = "account" | "sessions" | "keys";
const tabs: { id: Tab; label: string }[] = [
  { id: "account", label: "Account" },
  { id: "sessions", label: "Sessions" },
  { id: "keys", label: "API Keys" },
];
const activeTab = ref<Tab>("account");
const app = useAppStore();
const auth = useAuthStore();
const profile = useProfileSecurityStore();
const username = computed(() =>
  typeof auth.user?.username === "string" ? auth.user.username : "User",
);
const email = ref("");
const currentPassword = ref("");
const newPassword = ref("");
const confirmPassword = ref("");
const passwordError = computed(() => {
  if (!confirmPassword.value || newPassword.value === confirmPassword.value) {
    return "";
  }

  return "New passwords must match.";
});

watch(
  () => auth.user?.email,
  (value) => {
    email.value = typeof value === "string" ? value : "";
  },
  { immediate: true },
);
onMounted(() => void profile.refresh());

async function saveProfile() {
  await profile.updateEmail(email.value || null);
}

async function savePassword() {
  if (passwordError.value) {
    app.setError(passwordError.value);
    return;
  }

  await profile.changePassword(currentPassword.value, newPassword.value);
  currentPassword.value = "";
  newPassword.value = "";
  confirmPassword.value = "";
}

function clientLabel(agent?: string | null) {
  if (!agent) {
    return "Unknown client";
  }

  if (/Runinator/i.test(agent)) {
    return "Runinator desktop";
  }

  if (agent.includes("Firefox/")) {
    return "Firefox";
  }

  if (agent.includes("Edg/")) {
    return "Microsoft Edge";
  }

  if (agent.includes("Chrome/")) {
    return "Chrome";
  }

  if (agent.includes("Safari/")) {
    return "Safari";
  }

  return "Other client";
}

async function signOutSession(session: AuthSessionSummary) {
  if (window.confirm(session.current ? "Sign out this session?" : "Sign out this session?")) {
    await profile.revokeSession(session);
  }
}

async function signOutOthers() {
  if (window.confirm("Sign out every other session?")) {
    await profile.revokeOthers();
  }
}

const keyModalOpen = ref(false);
const selectedKey = ref<ApiKey | null>(null);
const keyName = ref("");
const keyScope = ref("");
const keyExpiry = ref("");
const keyActions = ref<Action[]>([]);
const keyDisabled = ref(false);
const selectedScopeActions = computed(
  () => profile.keyScopes.find((scope) => (scope.org_id ?? "") === keyScope.value)?.actions ?? [],
);

function resetKeyActions() {
  keyActions.value = [...selectedScopeActions.value];
}

function openCreateKey() {
  selectedKey.value = null;
  keyName.value = "";
  keyScope.value = profile.keyScopes[0]?.org_id ?? "";
  keyExpiry.value = "";
  keyDisabled.value = false;
  resetKeyActions();
  keyModalOpen.value = true;
}

function openEditKey(key: ApiKey) {
  selectedKey.value = key;
  keyName.value = key.name;
  keyScope.value = key.org_id ?? "";
  keyExpiry.value = key.expires_at ? key.expires_at.slice(0, 16) : "";
  keyDisabled.value = key.disabled;
  keyActions.value = [...key.action_ceiling];
  keyModalOpen.value = true;
}

function closeKeyModal() {
  keyModalOpen.value = false;
  selectedKey.value = null;
}

function isoExpiry() {
  return keyExpiry.value ? new Date(keyExpiry.value).toISOString() : null;
}

async function saveKey() {
  if (selectedKey.value?.id) {
    await profile.updateKey(selectedKey.value.id, keyName.value, isoExpiry(), keyDisabled.value);
  } else {
    if (!keyActions.value.length) {
      app.setError("Select at least one allowed action.");
      return;
    }

    await profile.createKey({
      name: keyName.value,
      orgId: keyScope.value === "" ? null : keyScope.value,
      expiresAt: isoExpiry(),
      actionCeiling: keyActions.value,
    });
  }

  closeKeyModal();
}

async function rotateSelectedKey() {
  if (
    selectedKey.value?.id &&
    window.confirm("Rotate this key? The old secret will stop working.")
  ) {
    await profile.rotateKey(selectedKey.value.id);
    closeKeyModal();
  }
}

async function revokeSelectedKey() {
  if (selectedKey.value?.id && window.confirm("Revoke this key permanently?")) {
    await profile.revokeKey(selectedKey.value.id);
    closeKeyModal();
  }
}

function scopeName(orgId?: string | null) {
  return (
    profile.keyScopes.find((scope) => scope.org_id === (orgId ?? null))?.name ?? orgId ?? "Platform"
  );
}

async function copySecret() {
  const secret = profile.revealedApiKey?.secret;

  if (!secret) {
    return;
  }

  try {
    await navigator.clipboard.writeText(secret);
    app.setStatus("API key secret copied.");
  } catch {
    app.setError("Unable to copy API key secret.");
  }
}
</script>
