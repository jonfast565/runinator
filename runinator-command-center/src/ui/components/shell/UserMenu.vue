<template>
  <div ref="menuRef" class="relative">
    <button
      type="button"
      class="btn flex items-center gap-2"
      aria-haspopup="menu"
      :aria-expanded="open"
      :title="email || username"
      @click="toggle"
    >
      <span
        class="inline-flex size-[22px] items-center justify-center rounded-full bg-accent text-[11px] font-semibold text-white"
        aria-hidden="true"
        >{{ initials }}</span
      >
      <span class="max-w-[140px] overflow-hidden text-ellipsis whitespace-nowrap">{{
        username
      }}</span>
      <Icon
        name="arrow-down"
        :size="14"
        class="transition-transform duration-200 ease-out"
        :class="open ? 'rotate-180' : ''"
      />
    </button>
    <Transition name="menu">
      <div
        v-if="open"
        class="absolute top-[calc(100%+4px)] right-0 z-30 origin-top-right grid min-w-[200px] rounded-md border border-border-strong bg-surface p-1 shadow-modal"
        role="menu"
      >
        <div class="flex flex-col gap-0.5 border-b border-border-subtle px-2 pt-2 pb-2.5">
          <span class="text-[13px] font-semibold text-fg">{{ username }}</span>
          <span
            v-if="email"
            class="overflow-hidden text-ellipsis whitespace-nowrap text-xs text-fg-muted"
            >{{ email }}</span
          >
          <span
            v-if="isAdmin"
            class="mt-1 self-start rounded-pill bg-accent-soft px-1.5 py-px text-[10px] font-semibold tracking-[0.04em] text-accent-text uppercase"
            >Admin</span
          >
        </div>
        <button
          type="button"
          role="menuitem"
          class="btn btn-ghost mt-1 justify-start gap-2 border-transparent bg-transparent text-xs text-fg hover:bg-surface-hover"
          @click="signOut"
        >
          <Icon name="lock" :size="14" />
          <span>Sign out</span>
        </button>
      </div>
    </Transition>
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
