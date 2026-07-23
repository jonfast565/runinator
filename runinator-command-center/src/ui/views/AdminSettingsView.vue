<template>
  <section class="pane flex h-full flex-col overflow-auto">
    <div class="panel flex min-h-0 flex-1 flex-row gap-0 max-md:flex-col">
      <!-- embedded left nav: sections and their per-item subsections. -->
      <nav
        class="flex w-[220px] shrink-0 flex-col gap-2 border-r border-border py-4 pr-4 max-md:w-auto max-md:border-r-0 max-md:border-b max-md:pb-4 max-md:pr-0"
        aria-label="Settings sections"
      >
        <div
          class="px-2 pt-4 pb-1 text-[0.78rem] font-bold tracking-wide text-fg-muted uppercase"
        >
          Settings
        </div>

        <div class="flex flex-col gap-0.5">
          <button
            class="flex w-full cursor-pointer items-center gap-1.5 rounded-md border-0 bg-transparent px-2 py-2 text-left font-semibold text-fg hover:bg-surface-muted"
            type="button"
            :class="
              activeSection === 'display' && !selected ? 'bg-surface-muted text-fg' : ''
            "
            @click="selectSection('display')"
          >
            <span>Display</span>
          </button>
        </div>

        <div class="flex flex-col gap-0.5">
          <button
            class="flex w-full cursor-pointer items-center gap-1.5 rounded-md border-0 bg-transparent px-2 py-2 text-left font-semibold text-fg hover:bg-surface-muted"
            type="button"
            :aria-expanded="languagesOpen"
            @click="languagesOpen = !languagesOpen"
          >
            <Icon
              name="chevron-right"
              class="transition-transform duration-150 ease-in-out"
              :class="{ 'rotate-90': languagesOpen }"
            />
            <span>Foreign Languages</span>
            <span
              class="ml-auto rounded-[10px] border border-border bg-surface-muted px-1.5 py-px text-[0.74rem] text-fg-muted"
              >{{ settings.languages.length }}</span
            >
          </button>
          <ul v-show="languagesOpen" class="m-0 flex list-none flex-col gap-0.5 py-0 pl-[18px]">
            <li v-for="runtime in settings.languages" :key="runtime.language">
              <button
                class="flex w-full cursor-pointer items-center justify-between gap-2 rounded-r-md border-0 border-l-2 border-transparent bg-transparent px-2 py-1.5 text-left text-fg-muted hover:bg-surface-muted hover:text-fg"
                type="button"
                :class="
                  activeSection === 'languages' && selected === runtime.language
                    ? 'border-l-accent bg-surface-muted font-semibold text-fg'
                    : ''
                "
                @click="selectLanguage(runtime.language)"
              >
                <span>{{ runtime.label }}</span>
                <span class="text-[0.76rem] text-fg-muted">{{ runtime.language }}</span>
              </button>
            </li>
          </ul>
        </div>
      </nav>

      <div class="flex min-w-0 flex-1 flex-col gap-4 p-4">
        <!-- display preferences panel -->
        <template v-if="activeSection === 'display'">
          <header
            class="flex items-center justify-between gap-3 max-md:flex-col max-md:items-stretch"
          >
            <div>
              <h2 class="m-0 text-base font-semibold text-fg">Display</h2>
              <p class="mt-1 mb-0 text-fg-muted">
                Appearance and navigation preferences stored locally in your browser.
              </p>
            </div>
          </header>

          <div class="flex flex-col rounded-lg border border-border">
            <div
              class="flex items-center justify-between gap-6 border-b border-border-faint px-4 py-3.5 max-md:flex-col max-md:items-start max-md:gap-2.5"
            >
              <div class="flex flex-col gap-0.5">
                <span class="font-semibold">Theme</span>
                <span class="text-[0.82rem] text-fg-muted"
                  >Override the app color scheme. "System" follows your OS setting.</span
                >
              </div>
              <div class="flex gap-1.5">
                <label
                  v-for="opt in themeOptions"
                  :key="opt.value"
                  class="flex cursor-pointer items-center gap-1.5 whitespace-nowrap rounded-md border border-border-strong bg-surface px-3 py-1.5 select-none hover:border-border-hover hover:bg-surface-hover"
                  :class="
                    prefs.theme === opt.value
                      ? 'border-accent bg-accent-soft font-semibold text-accent-text'
                      : ''
                  "
                >
                  <input
                    class="hidden"
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

            <div
              class="flex items-center justify-between gap-6 px-4 py-3.5 max-md:flex-col max-md:items-start max-md:gap-2.5"
            >
              <div class="flex flex-col gap-0.5">
                <span class="font-semibold">Default page</span>
                <span class="text-[0.82rem] text-fg-muted"
                  >Which page opens when you launch the app.</span
                >
              </div>
              <select class="w-auto min-w-40" :value="prefs.defaultTab" @change="onDefaultTabChange">
                <option v-for="opt in tabOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </div>
          </div>
        </template>

        <!-- language runtime panel -->
        <template v-else-if="activeSection === 'languages'">
          <header
            class="flex items-center justify-between gap-3 max-md:flex-col max-md:items-stretch"
          >
            <div>
              <h2 class="m-0 text-base font-semibold text-fg">
                {{ activeLanguage ? activeLanguage.label : "Foreign Languages" }}
              </h2>
              <p class="mt-1 mb-0 text-fg-muted">
                Runtime configuration shared by workers and workflow execution.
              </p>
            </div>
            <button class="btn max-md:w-full max-md:justify-center" type="button" @click="settings.refresh">
              <Icon name="refresh" />
              <span>Refresh</span>
            </button>
          </header>

          <form
            v-if="activeLanguage"
            class="grid gap-3.5 rounded-lg border border-border p-4"
            @submit.prevent="settings.saveLanguage(activeLanguage.language)"
          >
            <header
              class="flex items-center justify-between gap-3 max-md:flex-col max-md:items-stretch"
            >
              <div>
                <h3 class="m-0 text-sm font-semibold text-fg">{{ activeLanguage.label }}</h3>
                <p class="mt-1 mb-0 flex flex-wrap gap-2 text-fg-muted">
                  <span class="text-[0.84rem] text-fg-muted">{{ activeLanguage.language }}</span>
                  <span v-if="activeLanguage.aliases.length" class="text-[0.84rem] text-fg-muted"
                    >aliases: {{ activeLanguage.aliases.join(", ") }}</span
                  >
                </p>
              </div>
              <span
                class="whitespace-nowrap rounded-md border border-border bg-surface-muted px-2 py-1.5 text-[0.84rem] text-fg-muted"
                >{{ activeLanguage.defaultImage }}</span
              >
            </header>
            <label class="grid gap-1.5">
              <span class="text-[0.84rem] font-semibold text-fg-muted">Docker Image</span>
              <input v-model="activeLanguage.image" :placeholder="activeLanguage.defaultImage" />
            </label>
            <label class="grid gap-1.5">
              <span class="text-[0.84rem] font-semibold text-fg-muted">Setup Script</span>
              <textarea
                v-model="activeLanguage.setup_script"
                class="min-h-[120px] resize-y font-mono"
                spellcheck="false"
                placeholder="apt-get update && apt-get install -y curl"
              />
            </label>
            <div
              class="flex items-center justify-between gap-3 max-md:flex-col max-md:items-stretch"
            >
              <span
                class="whitespace-nowrap rounded-md border border-border bg-surface-muted px-2 py-1.5 text-[0.84rem] text-fg-muted"
                >config.foreign_languages.{{ activeLanguage.language }}</span
              >
              <button class="btn btn-primary max-md:w-full max-md:justify-center" type="submit">
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
import { useAdminSettingsStore } from "../../ui/adapters/pinia/adminSettings";
import {
  DEFAULT_TAB_OPTIONS,
  useDisplayPreferencesStore,
  type AppTheme,
} from "../../ui/adapters/pinia/displayPreferences";

const settings = useAdminSettingsStore();
const prefs = useDisplayPreferencesStore();

type ActiveSection = "display" | "languages";
const activeSection = ref<ActiveSection>("display");
const languagesOpen = ref(true);
const selected = ref<string>(settings.languages[0].language);

const themeOptions: { value: AppTheme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const tabOptions = DEFAULT_TAB_OPTIONS;

const activeLanguage = computed(() =>
  activeSection.value === "languages"
    ? settings.languages.find((entry) => entry.language === selected.value)
    : undefined,
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
  if (!settings.loaded) {
    void settings.refresh();
  }
});
</script>

