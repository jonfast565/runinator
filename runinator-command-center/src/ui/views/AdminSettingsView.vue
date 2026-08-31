<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.admin-settings.split"
      :initial-first-pct="24"
      :min-first="200"
      :min-second="380"
      collapsible-first
      first-label="Settings"
      first-icon="settings"
      mobile-mode="toggle"
      :mobile-detail-active="detailActive"
    >
      <!-- section rail: the sections and their per-item subsections. -->
      <template #first>
        <aside class="panel flex min-h-0 flex-col">
          <div class="panel-toolbar">
            <h2 class="m-0 text-base font-semibold text-fg">Settings</h2>
            <button class="btn" :disabled="loading" @click="refreshAll">
              <LoadingSpinner v-if="loading" size="sm" label="Refreshing settings" />
              <Icon v-else name="refresh" />
              <span>Refresh</span>
            </button>
          </div>

          <nav
            class="-mx-1 flex min-h-0 flex-1 flex-col gap-2 overflow-auto px-1"
            aria-label="Settings sections"
          >
            <div class="flex flex-col gap-0.5">
              <button
                class="flex w-full cursor-pointer items-center gap-1.5 rounded-md border-0 bg-transparent px-2 py-2 text-left font-semibold text-fg hover:bg-surface-muted"
                type="button"
                :class="activeSection === 'display' ? 'bg-surface-muted text-fg' : ''"
                @click="selectSection('display')"
              >
                <span>Display</span>
              </button>
            </div>

            <div class="flex flex-col gap-0.5">
              <button
                class="flex w-full cursor-pointer items-center gap-1.5 rounded-md border-0 bg-transparent px-2 py-2 text-left font-semibold text-fg hover:bg-surface-muted"
                type="button"
                :aria-expanded="serverOpen"
                @click="serverOpen = !serverOpen"
              >
                <Icon
                  name="chevron-right"
                  class="transition-transform duration-150 ease-in-out"
                  :class="{ 'rotate-90': serverOpen }"
                />
                <span>Server</span>
                <span
                  class="ml-auto rounded-[10px] border border-border bg-surface-muted px-1.5 py-px text-[0.74rem] text-fg-muted"
                  >{{ settings.serverCatalog.length + settings.runtimeCatalog.length }}</span
                >
              </button>
              <ul v-show="serverOpen" class="m-0 flex list-none flex-col gap-0.5 py-0 pl-[18px]">
                <li v-if="!serverSections.length" class="px-2 py-1.5 text-[0.84rem] text-fg-muted">
                  Nothing loaded yet.
                </li>
                <li v-for="section in serverSections" :key="section">
                  <button
                    class="flex w-full cursor-pointer items-center rounded-r-md border-0 border-l-2 border-transparent bg-transparent px-2 py-1.5 text-left text-fg-muted hover:bg-surface-muted hover:text-fg"
                    type="button"
                    :class="
                      activeSection === 'server' && activeServerSection === section
                        ? 'border-l-accent bg-surface-muted font-semibold text-fg'
                        : ''
                    "
                    @click="selectServerSection(section)"
                  >
                    {{ section }}
                  </button>
                </li>
              </ul>
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
                      activeSection === 'languages' && selectedLanguage === runtime.language
                        ? 'border-l-accent bg-surface-muted font-semibold text-fg'
                        : ''
                    "
                    @click="selectLanguage(runtime.language)"
                  >
                    <span class="min-w-0 truncate">{{ runtime.label }}</span>
                    <span class="shrink-0 text-[0.76rem] text-fg-muted">{{
                      runtime.language
                    }}</span>
                  </button>
                </li>
              </ul>
            </div>
          </nav>
        </aside>
      </template>

      <template #second>
        <div class="panel details flex min-h-0 flex-col gap-4 overflow-auto">
          <MobileBackBar label="Back to settings" @back="detailActive = false" />

          <!-- display preferences panel -->
          <template v-if="activeSection === 'display'">
            <header class="flex items-center gap-1">
              <h2 class="m-0 text-base font-semibold text-fg">Display</h2>
              <HelpBubble
                text="These preferences are stored in this browser and apply to you alone."
                label="About display settings"
              />
            </header>

            <div class="flex flex-col rounded-lg border border-border">
              <div
                class="flex items-center justify-between gap-6 border-b border-border-faint px-4 py-3.5 max-md:flex-col max-md:items-start max-md:gap-2.5"
              >
                <div class="flex flex-col gap-0.5">
                  <span class="font-semibold">Theme</span>
                </div>
                <div class="flex flex-wrap gap-1.5">
                  <label
                    v-for="opt in themeOptions"
                    :key="opt.value"
                    class="flex cursor-pointer items-center gap-1.5 rounded-md border border-border-strong bg-surface px-3 py-1.5 whitespace-nowrap select-none hover:border-border-hover hover:bg-surface-hover"
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
                </div>
                <select
                  class="w-auto min-w-40"
                  :value="prefs.defaultTab"
                  @change="onDefaultTabChange"
                >
                  <option v-for="opt in tabOptions" :key="opt.value" :value="opt.value">
                    {{ opt.label }}
                  </option>
                </select>
              </div>
            </div>
          </template>

          <template v-else-if="activeSection === 'server'">
            <header class="flex items-center gap-1">
              <h2 class="m-0 text-base font-semibold text-fg">
                {{ activeServerSection || "Server" }}
              </h2>
              <HelpBubble
                text="Platform-wide values are validated by the server and picked up by the replicas that own them without a restart."
                label="About server settings"
              />
            </header>

            <EmptyState
              v-if="!activeServerDefinitions.length && !activeRuntimeDefinitions.length"
              icon="settings"
              :loading="loading"
              loading-message="Loading server settings…"
              title="No server settings in this section"
            />
            <form
              v-else
              class="grid max-w-3xl gap-3.5"
              @submit.prevent="settings.saveServerSettings"
            >
              <label
                v-for="definition in activeServerDefinitions"
                :key="definition.key"
                class="grid gap-1.5 rounded-lg border border-border p-4"
              >
                <span class="font-semibold text-fg">{{ definition.label }}</span>
                <span class="text-[0.84rem] text-fg-muted">{{ definition.description }}</span>
                <input
                  v-if="definition.kind === 'boolean'"
                  type="checkbox"
                  :checked="serverValue(definition.key) === true"
                  @change="
                    settings.updateServerSetting(
                      definition.key,
                      ($event.target as HTMLInputElement).checked,
                    )
                  "
                />
                <input
                  v-else
                  type="number"
                  required
                  step="1"
                  :min="definition.minimum"
                  :max="definition.maximum"
                  :value="serverValue(definition.key)"
                  @input="
                    settings.updateServerSetting(
                      definition.key,
                      Number(($event.target as HTMLInputElement).value),
                    )
                  "
                />
                <span
                  v-if="definition.kind !== 'boolean'"
                  class="text-[0.78rem] text-fg-muted"
                >
                  Usual {{ definition.usual_minimum.toLocaleString() }}–{{
                    definition.usual_maximum.toLocaleString()
                  }}
                  {{ definition.unit }}; valid {{ definition.minimum.toLocaleString() }}–{{
                    definition.maximum.toLocaleString()
                  }}; default {{ definition.default.toLocaleString() }}.
                </span>
              </label>
              <article
                v-for="definition in activeRuntimeDefinitions"
                :key="definition.key"
                class="grid gap-1.5 rounded-lg border border-border p-4"
              >
                <span class="font-semibold text-fg">{{ definition.label }}</span>
                <span class="text-[0.84rem] text-fg-muted">{{ definition.description }}</span>
                <code class="rounded bg-surface-muted px-2 py-1.5 text-sm break-all">{{
                  definition.value
                }}</code>
                <span class="text-[0.78rem] text-fg-muted"
                  >Source: {{ definition.source
                  }}<template v-if="definition.restart_required">
                    · restart required to change</template
                  ><template v-if="definition.sensitive"> · sensitive value hidden</template></span
                >
              </article>
              <button
                v-if="activeServerDefinitions.length"
                class="btn btn-primary justify-self-start"
                type="submit"
              >
                <Icon name="save" />
                <span>Save server settings</span>
              </button>
            </form>
          </template>

          <!-- language runtime panel -->
          <template v-else-if="activeSection === 'languages'">
            <header class="flex items-center gap-1">
              <h2 class="m-0 text-base font-semibold text-fg">
                {{ activeLanguage ? activeLanguage.label : "Foreign Languages" }}
              </h2>
              <HelpBubble label="About foreign language runtimes">
                Configure the image, toolchain, environment, and resource envelope
                <code>std.code</code> uses to run this language. Environment values are not secret.
              </HelpBubble>
            </header>

            <EmptyState
              v-if="!activeLanguage"
              icon="box"
              title="Pick a language"
              :loading="loading"
              loading-message="Loading language runtimes…"
            />
            <form
              v-else
              class="grid max-w-3xl gap-3.5 rounded-lg border border-border p-4"
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
                  class="rounded-md border border-border bg-surface-muted px-2 py-1.5 text-[0.84rem] whitespace-nowrap text-fg-muted"
                  >{{ activeLanguage.defaultImage }}</span
                >
              </header>
              <label class="grid gap-1.5">
                <span class="text-[0.84rem] font-semibold text-fg-muted">Docker image</span>
                <input
                  :value="activeLanguage.image"
                  required
                  :placeholder="activeLanguage.defaultImage"
                  @input="onLanguageField('image', $event)"
                />
              </label>
              <div class="grid gap-3 md:grid-cols-2">
                <label class="grid gap-1.5">
                  <span class="text-[0.84rem] font-semibold text-fg-muted">Executable</span>
                  <input
                    :value="activeLanguage.executable"
                    required
                    :placeholder="activeLanguage.defaultExecutable"
                    @input="onLanguageField('executable', $event)"
                  />
                </label>
                <label class="grid gap-1.5">
                  <span class="text-[0.84rem] font-semibold text-fg-muted">Environment</span>
                  <textarea
                    :value="activeLanguage.environment_text"
                    class="min-h-[84px] resize-y font-mono"
                    spellcheck="false"
                    placeholder="NAME=value (one per line)"
                    @input="onLanguageField('environment_text', $event)"
                  />
                </label>
                <label class="grid gap-1.5">
                  <span class="text-[0.84rem] font-semibold text-fg-muted">Build arguments</span>
                  <textarea
                    :value="activeLanguage.build_args_text"
                    class="min-h-[84px] resize-y font-mono"
                    spellcheck="false"
                    placeholder="One exact argument per line"
                    @input="onLanguageField('build_args_text', $event)"
                  />
                </label>
                <label class="grid gap-1.5">
                  <span class="text-[0.84rem] font-semibold text-fg-muted">Run arguments</span>
                  <textarea
                    :value="activeLanguage.run_args_text"
                    class="min-h-[84px] resize-y font-mono"
                    spellcheck="false"
                    placeholder="One exact argument per line"
                    @input="onLanguageField('run_args_text', $event)"
                  />
                </label>
              </div>
              <fieldset class="grid gap-3 rounded-md border border-border p-3">
                <legend class="px-1 text-[0.84rem] font-semibold text-fg-muted">Resource limits</legend>
                <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                  <label v-for="limit in languageLimits" :key="limit.field" class="grid gap-1.5">
                    <span class="text-[0.84rem] font-semibold text-fg-muted">{{ limit.label }}</span>
                    <input
                      :value="activeLanguage[limit.field]"
                      type="number"
                      min="1"
                      step="1"
                      required
                      @input="onLanguageLimit(limit.field, $event)"
                    />
                  </label>
                </div>
              </fieldset>
              <label class="grid gap-1.5">
                <span class="text-[0.84rem] font-semibold text-fg-muted">Setup script</span>
                <textarea
                  :value="activeLanguage.setup_script"
                  class="min-h-[120px] resize-y font-mono"
                  spellcheck="false"
                  placeholder="apt-get update && apt-get install -y curl"
                  @input="onLanguageField('setup_script', $event)"
                />
              </label>
              <div
                class="flex items-center justify-between gap-3 max-md:flex-col max-md:items-stretch"
              >
                <span
                  class="rounded-md border border-border bg-surface-muted px-2 py-1.5 text-[0.84rem] whitespace-nowrap text-fg-muted"
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
      </template>
    </SplitPane>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import EmptyState from "../components/shared/EmptyState.vue";
import HelpBubble from "../components/shared/HelpBubble.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import { useAdminSettingsStore } from "../../ui/adapters/pinia/adminSettings";
import {
  DEFAULT_TAB_OPTIONS,
  useDisplayPreferencesStore,
  type AppTheme,
} from "../../ui/adapters/pinia/displayPreferences";
import { useOperationLoading } from "../composables/useOperationLoading";

const settings = useAdminSettingsStore();
const prefs = useDisplayPreferencesStore();
const { isLoading: loading } = useOperationLoading([
  "Loading admin settings",
  "Loading server settings",
]);

type ActiveSection = "display" | "server" | "languages";
const activeSection = ref<ActiveSection>("display");
const languagesOpen = ref(true);
const serverOpen = ref(true);
// on mobile the split shows one pane at a time; the rail is the list, the panel is the detail.
const detailActive = ref(false);
const selectedLanguage = ref<string>(settings.languages[0]?.language ?? "");
const selectedServerSection = ref("");

const themeOptions: { value: AppTheme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const tabOptions = DEFAULT_TAB_OPTIONS;

const activeLanguage = computed(() =>
  activeSection.value === "languages"
    ? settings.languages.find((entry) => entry.language === selectedLanguage.value)
    : undefined,
);

const serverSections = computed(() => [
  ...new Set([
    ...settings.serverCatalog.map((definition) => definition.section),
    ...settings.runtimeCatalog.map((definition) => definition.section),
  ]),
]);

// the catalog arrives after mount, so fall back to its first section until one is picked.
const activeServerSection = computed(() =>
  serverSections.value.includes(selectedServerSection.value)
    ? selectedServerSection.value
    : (serverSections.value[0] ?? ""),
);

const activeServerDefinitions = computed(() =>
  settings.serverCatalog.filter((definition) => definition.section === activeServerSection.value),
);

const activeRuntimeDefinitions = computed(() =>
  settings.runtimeCatalog.filter((definition) => definition.section === activeServerSection.value),
);

function selectSection(section: ActiveSection) {
  activeSection.value = section;
  detailActive.value = true;
}

function selectServerSection(section: string) {
  selectedServerSection.value = section;
  selectSection("server");
}

// the catalog can name a section the values payload has not loaded yet, so read defensively.
function serverValue(key: string) {
  const [section, name] = key.split(".");
  const values = settings.serverValues as Record<
    string,
    Record<string, number | boolean> | undefined
  >;

  return values[section]?.[name] ?? "";
}

function selectLanguage(language: string) {
  selectedLanguage.value = language;
  selectSection("languages");
}

function onLanguageField(
  field: "image" | "setup_script" | "environment_text" | "executable" | "build_args_text" | "run_args_text",
  event: Event,
) {
  if (!activeLanguage.value) {
    return;
  }

  const target = event.target as HTMLInputElement | HTMLTextAreaElement;
  settings.updateLanguageField(activeLanguage.value.language, field, target.value);
}

const languageLimits = [
  { field: "memory_mb", label: "Memory (MiB)" },
  { field: "cpu_millis", label: "CPU (millicores)" },
  { field: "pids", label: "Processes" },
  { field: "tmpfs_mb", label: "/tmp (MiB)" },
  { field: "max_output_bytes", label: "Output bytes / stream" },
] as const;

function onLanguageLimit(field: (typeof languageLimits)[number]["field"], event: Event) {
  if (!activeLanguage.value) {
    return;
  }

  settings.updateLanguageLimit(
    activeLanguage.value.language,
    field,
    Number((event.target as HTMLInputElement).value),
  );
}

function onDefaultTabChange(event: Event) {
  prefs.setDefaultTab((event.target as HTMLSelectElement).value);
}

function refreshAll() {
  void settings.refreshServerSettings();
  void settings.refresh();
}

onMounted(() => {
  void settings.refreshServerSettings();

  if (!settings.loaded) {
    void settings.refresh();
  }
});
</script>
