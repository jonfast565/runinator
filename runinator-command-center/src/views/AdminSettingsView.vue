<template>
  <section class="pane admin-settings-pane">
    <div class="settings-shell panel">
      <header class="settings-header">
        <div>
          <h2>Settings</h2>
          <p>Runtime configuration shared by workers and workflow execution.</p>
        </div>
        <button class="btn" type="button" @click="settings.refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </button>
      </header>

      <section class="settings-section">
        <header class="section-header">
          <div>
            <h3>Foreign Languages</h3>
            <p>Configure the container runtime used by WDL foreign compute nodes.</p>
          </div>
          <span class="section-badge">{{ settings.languages.length }} languages</span>
        </header>

        <div class="runtime-grid">
          <form
            v-for="runtime in settings.languages"
            :key="runtime.language"
            class="runtime-form"
            @submit.prevent="settings.saveLanguage(runtime.language)"
          >
            <header class="runtime-header">
              <div>
                <h4>{{ runtime.label }}</h4>
                <p>
                  <span>{{ runtime.language }}</span>
                  <span v-if="runtime.aliases.length">aliases: {{ runtime.aliases.join(", ") }}</span>
                </p>
              </div>
              <span class="runtime-default">{{ runtime.defaultImage }}</span>
            </header>
            <label>
              <span>Docker Image</span>
              <input v-model="runtime.image" :placeholder="runtime.defaultImage" />
            </label>
            <label class="setup-field">
              <span>Setup Script</span>
              <textarea
                v-model="runtime.setup_script"
                spellcheck="false"
                placeholder="apt-get update && apt-get install -y curl"
              />
            </label>
            <div class="runtime-actions">
              <span class="runtime-ref">config.foreign_languages.{{ runtime.language }}</span>
              <button class="btn btn-primary" type="submit">
                <Icon name="save" />
                <span>Save</span>
              </button>
            </div>
          </form>
        </div>
      </section>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted } from "vue";
import Icon from "../components/shared/Icon.vue";
import { useAdminSettingsStore } from "../stores/adminSettings";

const settings = useAdminSettingsStore();

onMounted(() => {
  if (!settings.loaded) void settings.refresh();
});
</script>

<style scoped>
.admin-settings-pane {
  overflow: auto;
}

.settings-shell {
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin: 0 auto;
  max-width: 1120px;
}

.settings-header,
.section-header,
.runtime-actions,
.runtime-header {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
}

.settings-header h2,
.section-header h3,
.runtime-header h4 {
  margin: 0;
}

.settings-header p,
.section-header p,
.runtime-header p {
  color: var(--text-muted);
  margin: 4px 0 0;
}

.settings-section {
  border: 1px solid var(--border);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 16px;
}

.section-badge,
.runtime-ref,
.runtime-default {
  background: var(--surface-muted);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-muted);
  font-size: 0.84rem;
  padding: 6px 8px;
  white-space: nowrap;
}

.runtime-grid {
  display: grid;
  gap: 12px;
}

.runtime-form {
  border: 1px solid var(--border);
  border-radius: 8px;
  display: grid;
  gap: 12px;
  padding: 14px;
}

.runtime-form label {
  display: grid;
  gap: 6px;
}

.runtime-form label > span,
.runtime-header p span {
  color: var(--text-muted);
  font-size: 0.84rem;
  font-weight: 600;
}

.runtime-header p {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.setup-field textarea {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  min-height: 120px;
  resize: vertical;
}

@media (max-width: 760px) {
  .settings-header,
  .section-header,
  .runtime-actions,
  .runtime-header {
    align-items: stretch;
    flex-direction: column;
  }

  .runtime-actions .btn {
    justify-content: center;
    width: 100%;
  }
}
</style>
