<template>
  <section class="pane admin-settings-pane">
    <div class="settings-shell panel">
      <!-- embedded left nav: sections and their per-item subsections. -->
      <nav class="settings-nav" aria-label="Settings sections">
        <div class="settings-nav-title">Settings</div>

        <div class="nav-section">
          <button
            class="nav-section-header"
            type="button"
            :class="{ 'nav-section-header--active': activeSection === 'display' && !selected }"
            @click="selectSection('display')"
          >
            <span>Display</span>
          </button>
        </div>

        <div class="nav-section">
          <button
            class="nav-section-header"
            type="button"
            :aria-expanded="languagesOpen"
            @click="languagesOpen = !languagesOpen"
          >
            <Icon name="chevron-right" class="nav-chevron" :class="{ open: languagesOpen }" />
            <span>Foreign Languages</span>
            <span class="nav-count">{{ settings.languages.length }}</span>
          </button>
          <ul v-show="languagesOpen" class="nav-subsections">
            <li v-for="runtime in settings.languages" :key="runtime.language">
              <button
                class="nav-subitem"
                type="button"
                :class="{ active: activeSection === 'languages' && selected === runtime.language }"
                @click="selectLanguage(runtime.language)"
              >
                <span>{{ runtime.label }}</span>
                <span class="nav-subitem-ref">{{ runtime.language }}</span>
              </button>
            </li>
          </ul>
        </div>
      </nav>

      <div class="settings-content">
        <!-- display preferences panel -->
        <template v-if="activeSection === 'display'">
          <header class="settings-header">
            <div>
              <h2>Display</h2>
              <p>Appearance and navigation preferences stored locally in your browser.</p>
            </div>
          </header>

          <div class="settings-card">
            <div class="settings-card-row">
              <div class="settings-card-label">
                <span>Theme</span>
                <span class="settings-card-hint">Override the app color scheme. "System" follows your OS setting.</span>
              </div>
              <div class="theme-options">
                <label
                  v-for="opt in themeOptions"
                  :key="opt.value"
                  class="theme-option"
                  :class="{ active: prefs.theme === opt.value }"
                >
                  <input
                    type="radio"
                    name="theme"
                    :value="opt.value"
                    :checked="prefs.theme === opt.value"
                    @change="prefs.setTheme(opt.value)"
                  />
                  {{ opt.label }}
                </label>
              </div>
            </div>

            <div class="settings-card-row">
              <div class="settings-card-label">
                <span>Default page</span>
                <span class="settings-card-hint">Which page opens when you launch the app.</span>
              </div>
              <select :value="prefs.defaultTab" @change="onDefaultTabChange">
                <option v-for="opt in tabOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
              </select>
            </div>
          </div>
        </template>

        <!-- language runtime panel -->
        <template v-else-if="activeSection === 'languages'">
          <header class="settings-header">
            <div>
              <h2>{{ activeLanguage ? activeLanguage.label : "Foreign Languages" }}</h2>
              <p>Runtime configuration shared by workers and workflow execution.</p>
            </div>
            <button class="btn" type="button" @click="settings.refresh">
              <Icon name="refresh" />
              <span>Refresh</span>
            </button>
          </header>

          <form
            v-if="activeLanguage"
            class="language-form"
            @submit.prevent="settings.saveLanguage(activeLanguage.language)"
          >
            <header class="language-form-header">
              <div>
                <h3>{{ activeLanguage.label }}</h3>
                <p>
                  <span>{{ activeLanguage.language }}</span>
                  <span v-if="activeLanguage.aliases.length">aliases: {{ activeLanguage.aliases.join(", ") }}</span>
                </p>
              </div>
              <span class="lang-badge">{{ activeLanguage.defaultImage }}</span>
            </header>
            <label>
              <span>Docker Image</span>
              <input v-model="activeLanguage.image" :placeholder="activeLanguage.defaultImage" />
            </label>
            <label class="setup-field">
              <span>Setup Script</span>
              <textarea
                v-model="activeLanguage.setup_script"
                spellcheck="false"
                placeholder="apt-get update && apt-get install -y curl"
              />
            </label>
            <div class="form-actions">
              <span class="form-ref">config.foreign_languages.{{ activeLanguage.language }}</span>
              <button class="btn btn-primary" type="submit">
                <Icon name="save" />
                <span>Save</span>
              </button>
            </div>
          </form>
        </template>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import Icon from "../components/shared/Icon.vue";
import { useAdminSettingsStore } from "../stores/adminSettings";
import { DEFAULT_TAB_OPTIONS, useDisplayPreferencesStore, type AppTheme } from "../stores/displayPreferences";

const settings = useAdminSettingsStore();
const prefs = useDisplayPreferencesStore();

type ActiveSection = "display" | "languages";
const activeSection = ref<ActiveSection>("display");
const languagesOpen = ref(true);
const selected = ref<string>(settings.languages[0]?.language ?? "");

const themeOptions: { value: AppTheme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" }
];

const tabOptions = DEFAULT_TAB_OPTIONS;

const activeLanguage = computed(() =>
  activeSection.value === "languages" ? settings.languages.find((entry) => entry.language === selected.value) : undefined
);

function selectSection(section: ActiveSection) {
  activeSection.value = section;
  selected.value = "";
}

function selectLanguage(language: string) {
  activeSection.value = "languages";
  selected.value = language;
}

function onDefaultTabChange(event: Event) {
  prefs.setDefaultTab((event.target as HTMLSelectElement).value);
}

onMounted(() => {
  if (!settings.loaded) void settings.refresh();
});
</script>

<style scoped>
.admin-settings-pane {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: auto;
}

.settings-shell {
  display: flex;
  flex-direction: row;
  flex: 1;
  gap: 0;
  min-height: 0;
}

/* embedded left nav. */
.settings-nav {
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  gap: 8px;
  padding: 16px 16px 16px 0;
  width: 220px;
}

.settings-nav-title {
  color: var(--text-muted);
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  padding: 16px 8px 4px;
  text-transform: uppercase;
}

.nav-section {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav-section-header {
  align-items: center;
  background: transparent;
  border: none;
  border-radius: 6px;
  color: var(--text);
  cursor: pointer;
  display: flex;
  font-weight: 600;
  gap: 6px;
  padding: 8px;
  text-align: left;
  width: 100%;
}

.nav-section-header:hover {
  background: var(--surface-muted);
}

.nav-section-header--active {
  background: var(--surface-muted);
  color: var(--text);
}

.nav-chevron {
  transition: transform 0.15s ease;
}

.nav-chevron.open {
  transform: rotate(90deg);
}

.nav-count {
  background: var(--surface-muted);
  border: 1px solid var(--border);
  border-radius: 10px;
  color: var(--text-muted);
  font-size: 0.74rem;
  margin-left: auto;
  padding: 1px 7px;
}

.nav-subsections {
  display: flex;
  flex-direction: column;
  gap: 2px;
  list-style: none;
  margin: 0;
  padding: 0 0 0 18px;
}

.nav-subitem {
  align-items: center;
  background: transparent;
  border: none;
  border-left: 2px solid transparent;
  border-radius: 0 6px 6px 0;
  color: var(--text-muted);
  cursor: pointer;
  display: flex;
  gap: 8px;
  justify-content: space-between;
  padding: 7px 8px;
  text-align: left;
  width: 100%;
}

.nav-subitem:hover {
  background: var(--surface-muted);
  color: var(--text);
}

.nav-subitem.active {
  background: var(--surface-muted);
  border-left-color: var(--accent, var(--text));
  color: var(--text);
  font-weight: 600;
}

.nav-subitem-ref {
  color: var(--text-muted);
  font-size: 0.76rem;
}

.settings-content {
  display: flex;
  flex-direction: column;
  flex: 1;
  gap: 16px;
  min-width: 0;
  padding: 16px;
}

.settings-header,
.language-form-header,
.form-actions {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
}

.settings-header h2,
.language-form-header h3 {
  margin: 0;
}

.settings-header p,
.language-form-header p {
  color: var(--text-muted);
  margin: 4px 0 0;
}

.language-form-header p {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.language-form-header p span {
  color: var(--text-muted);
  font-size: 0.84rem;
}

.lang-badge,
.form-ref {
  background: var(--surface-muted);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-muted);
  font-size: 0.84rem;
  padding: 6px 8px;
  white-space: nowrap;
}

.language-form {
  border: 1px solid var(--border);
  border-radius: 8px;
  display: grid;
  gap: 14px;
  padding: 16px;
}

.language-form label {
  display: grid;
  gap: 6px;
}

.language-form label > span {
  color: var(--text-muted);
  font-size: 0.84rem;
  font-weight: 600;
}

.setup-field textarea {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  min-height: 120px;
  resize: vertical;
}

/* display preferences card */
.settings-card {
  border: 1px solid var(--border);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 0;
}

.settings-card-row {
  align-items: center;
  border-bottom: 1px solid var(--border-faint);
  display: flex;
  gap: 24px;
  justify-content: space-between;
  padding: 14px 16px;
}

.settings-card-row:last-child {
  border-bottom: none;
}

.settings-card-label {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.settings-card-label > span:first-child {
  font-weight: 600;
}

.settings-card-hint {
  color: var(--text-muted);
  font-size: 0.82rem;
}

.theme-options {
  display: flex;
  gap: 6px;
}

.theme-option {
  align-items: center;
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: 6px;
  cursor: pointer;
  display: flex;
  gap: 6px;
  padding: 6px 12px;
  user-select: none;
  white-space: nowrap;
}

.theme-option:hover {
  background: var(--surface-hover);
  border-color: var(--border-hover);
}

.theme-option.active {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent-text);
  font-weight: 600;
}

.theme-option input[type="radio"] {
  display: none;
}

.settings-card-row select {
  width: auto;
  min-width: 160px;
}

@media (max-width: 760px) {
  .settings-shell {
    flex-direction: column;
  }

  .settings-nav {
    border-bottom: 1px solid var(--border);
    border-right: none;
    padding: 0 0 16px;
    width: auto;
  }

  .settings-header,
  .language-form-header,
  .form-actions {
    align-items: stretch;
    flex-direction: column;
  }

  .form-actions .btn {
    justify-content: center;
    width: 100%;
  }

  .settings-card-row {
    align-items: flex-start;
    flex-direction: column;
    gap: 10px;
  }
}
</style>
