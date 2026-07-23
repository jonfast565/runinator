<template>
  <div class="flex min-h-screen min-h-dvh items-center justify-center bg-app">
    <form
      class="grid w-[min(360px,calc(100vw-32px))] gap-3 rounded-lg border border-border bg-surface p-7 shadow-modal"
      @submit.prevent="submit"
    >
      <h1 class="m-0 text-xl text-fg">Runinator</h1>
      <p class="m-0 mb-2 text-[13px] text-fg-muted">Sign in to continue</p>
      <label class="grid gap-1 text-xs font-semibold text-fg-subtle">
        Username
        <input
          v-model="username"
          class="rounded-md border border-border-strong px-2.5 py-2 font-inherit"
          autocomplete="username"
          autofocus
        />
      </label>
      <label class="grid gap-1 text-xs font-semibold text-fg-subtle">
        Password
        <input
          v-model="password"
          class="rounded-md border border-border-strong px-2.5 py-2 font-inherit"
          type="password"
          autocomplete="current-password"
        />
      </label>
      <p v-if="auth.error" class="m-0 text-xs text-danger-fg">{{ auth.error }}</p>
      <button
        class="btn btn-primary mt-1 gap-2 px-3 py-2.5 font-semibold disabled:cursor-not-allowed disabled:opacity-60"
        type="submit"
        :disabled="submitting || !username || !password"
      >
        <LoadingSpinner v-if="submitting" size="sm" label="Signing in" />
        {{ submitting ? "Signing in…" : "Sign in" }}
      </button>
    </form>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import { useAuthStore } from "../../ui/adapters/pinia/auth";

const auth = useAuthStore();
const username = ref("");
const password = ref("");
const submitting = ref(false);

async function submit() {
  submitting.value = true;

  try {
    await auth.signIn(username.value, password.value);
  } finally {
    submitting.value = false;
  }
}
</script>
