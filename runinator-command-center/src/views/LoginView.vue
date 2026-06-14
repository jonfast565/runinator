<template>
  <div class="login-backdrop">
    <form class="login-card" @submit.prevent="submit">
      <h1>Runinator</h1>
      <p class="login-sub">Sign in to continue</p>
      <label>
        Username
        <input v-model="username" autocomplete="username" autofocus />
      </label>
      <label>
        Password
        <input v-model="password" type="password" autocomplete="current-password" />
      </label>
      <p v-if="auth.error" class="login-error">{{ auth.error }}</p>
      <button class="login-submit" type="submit" :disabled="submitting || !username || !password">
        {{ submitting ? "Signing in…" : "Sign in" }}
      </button>
    </form>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useAuthStore } from "../stores/auth";

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

<style scoped>
.login-backdrop {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  background: #f3f6f9;
}

.login-card {
  display: grid;
  gap: 12px;
  width: min(360px, calc(100vw - 32px));
  padding: 28px;
  border: 1px solid #ccd4dd;
  border-radius: 10px;
  background: #ffffff;
  box-shadow: 0 12px 32px rgba(15, 23, 42, 0.12);
}

.login-card h1 {
  margin: 0;
  font-size: 20px;
  color: #17202a;
}

.login-sub {
  margin: 0 0 8px;
  color: #66717e;
  font-size: 13px;
}

.login-card label {
  display: grid;
  gap: 4px;
  color: #3b4652;
  font-size: 12px;
  font-weight: 600;
}

.login-card input {
  padding: 8px 10px;
  border: 1px solid #ccd4dd;
  border-radius: 6px;
  font: inherit;
}

.login-error {
  margin: 0;
  color: #c53030;
  font-size: 12px;
}

.login-submit {
  margin-top: 4px;
  padding: 9px 12px;
  border: 0;
  border-radius: 6px;
  background: #17202a;
  color: #ffffff;
  font-weight: 600;
  cursor: pointer;
}

.login-submit:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
</style>
