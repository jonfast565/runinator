<template>
  <div ref="menuRef" class="user-menu">
    <button
      type="button"
      class="btn user-trigger"
      aria-haspopup="menu"
      :aria-expanded="open"
      :title="email || username"
      @click="toggle"
    >
      <span class="user-avatar" aria-hidden="true">{{ initials }}</span>
      <span class="user-name">{{ username }}</span>
      <Icon name="arrow-down" :size="14" />
    </button>
    <div v-if="open" class="user-menu-panel" role="menu">
      <div class="user-info">
        <span class="user-info-name">{{ username }}</span>
        <span v-if="email" class="user-info-email">{{ email }}</span>
        <span v-if="isAdmin" class="user-info-badge">Admin</span>
      </div>
      <button type="button" role="menuitem" class="btn btn-ghost user-menu-item" @click="signOut">
        <Icon name="lock" :size="14" />
        <span>Sign out</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import Icon from "../shared/Icon.vue";
import { useAuthStore } from "../../../ui/adapters/pinia/auth";

const auth = useAuthStore();
const menuRef = ref<HTMLElement | null>(null);
const open = ref(false);

const username = computed(() => {
  const value = auth.user?.username;
  return typeof value === "string" && value.length > 0 ? value : "User";
});

const email = computed(() => {
  const value = auth.user?.email;
  return typeof value === "string" ? value : "";
});

const isAdmin = computed(() => auth.user?.is_admin === true);

const initials = computed(() => {
  const source = username.value.trim();
  const parts = source.split(/[\s._-]+/).filter(Boolean);

  if (parts.length === 0) {
    return "?";
  }

  if (parts.length === 1) {
    return parts[0].slice(0, 2).toUpperCase();
  }

  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
});

function toggle() {
  open.value = !open.value;
}

function close() {
  open.value = false;
}

async function signOut() {
  close();
  await auth.signOut();
}

function onDocumentPointerDown(event: PointerEvent) {
  const target = event.target;

  if (!(target instanceof Node)) {
    return;
  }

  if (menuRef.value?.contains(target)) {
    return;
  }

  close();
}

function onDocumentKeyDown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    close();
  }
}

// dismissal listeners live only while the menu is open, so they are not always-on global handlers.
watch(open, (isOpen) => {
  if (isOpen) {
    document.addEventListener("pointerdown", onDocumentPointerDown);
    document.addEventListener("keydown", onDocumentKeyDown);
  } else {
    document.removeEventListener("pointerdown", onDocumentPointerDown);
    document.removeEventListener("keydown", onDocumentKeyDown);
  }
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocumentPointerDown);
  document.removeEventListener("keydown", onDocumentKeyDown);
});
</script>

<style scoped>
.user-menu {
  position: relative;
}

.user-trigger {
  display: flex;
  align-items: center;
  gap: 8px;
}

.user-avatar {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: var(--accent);
  color: #ffffff;
  font-size: 11px;
  font-weight: 600;
}

.user-name {
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.user-menu-panel {
  position: absolute;
  z-index: 30;
  top: calc(100% + 4px);
  right: 0;
  display: grid;
  min-width: 200px;
  padding: 4px;
  border: 1px solid var(--border-strong);
  border-radius: 6px;
  background: var(--surface);
  box-shadow: var(--shadow-modal);
}

.user-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 8px 10px;
  border-bottom: 1px solid var(--border-subtle);
}

.user-info-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--text);
}

.user-info-email {
  font-size: 12px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.user-info-badge {
  align-self: flex-start;
  margin-top: 4px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--accent-soft);
  color: var(--accent-text);
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.user-menu-item {
  justify-content: flex-start;
  gap: 8px;
  margin-top: 4px;
  border-color: transparent;
  background: transparent;
  color: var(--text);
  font-size: 12px;
}

.user-menu-item:hover:not(:disabled) {
  background: var(--surface-hover);
}
</style>
