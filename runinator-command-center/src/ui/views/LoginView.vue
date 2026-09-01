<template>
  <div class="flex min-h-screen min-h-dvh items-center justify-center bg-app">
    <form
      ref="loginForm"
      class="grid w-[min(360px,calc(100vw-32px))] gap-3 rounded-lg border border-border bg-surface p-7 shadow-modal"
      @input.capture="clearFieldError"
      @submit.prevent="submitValidated"
    >
      <div class="flex items-center gap-2">
        <BrandMark />
        <h1 class="m-0 text-xl text-fg">Runinator</h1>
        <HelpBubble
          text="Sign in with your Runinator account. Your available pages and actions are limited by the roles assigned to that account."
          label="About signing in"
        />
      </div>
      <label class="grid gap-1 text-xs font-semibold text-fg-subtle">
        Username
        <input
          v-model="username"
          class="rounded-md border border-border-strong px-2.5 py-2 font-inherit"
          autocomplete="username"
          autofocus
          required
          maxlength="256"
        />
      </label>
      <label class="grid gap-1 text-xs font-semibold text-fg-subtle">
        Password
        <input
          v-model="password"
          class="rounded-md border border-border-strong px-2.5 py-2 font-inherit"
          type="password"
          autocomplete="current-password"
          required
          maxlength="16384"
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
import HelpBubble from "../components/shared/HelpBubble.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import BrandMark from "../components/shell/BrandMark.vue";
import { useAuthStore } from "../../ui/adapters/pinia/auth";
import { validateFormControl, validateFormControls } from "../composables/form-validation";

const auth = useAuthStore();
const username = ref("");
const password = ref("");
const submitting = ref(false);
const loginForm = ref<HTMLFormElement | null>(null);

function clearFieldError(event: Event) {
  const control = event.target;

  if (!(control instanceof HTMLInputElement)) {
    return;
  }

  if (validateFormControl(control)) {
    control.removeAttribute("aria-invalid");
  }
}

async function submitValidated() {
  const invalid = loginForm.value ? validateFormControls(loginForm.value) : null;

  if (invalid) {
    invalid.setAttribute("aria-invalid", "true");
    invalid.focus();
    invalid.reportValidity();
    return;
  }

  await submit();
}

async function submit() {
  submitting.value = true;

  try {
    await auth.signIn(username.value, password.value);
  } finally {
    submitting.value = false;
  }
}
</script>
