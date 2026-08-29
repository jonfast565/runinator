<template>
  <Modal
    title="Publish a function package"
    width="min(680px, calc(100vw - 32px))"
    @close="emit('close')"
  >
    <template #help>
      Publishing takes a built package archive and the manifest that describes it. The archive is
      stored under the SHA-256 of its bytes, so the version is pinned to exactly these bytes. To
      build one from a working tree, run
      <code>runinatorctl functions publish &lt;path&gt;</code>.
    </template>

    <section class="form-section">
      <h3>Archive</h3>
      <label class="grid gap-1 text-xs text-fg-subtle">
        Package .zip
        <input type="file" accept=".zip,application/zip" @change="onArchive" />
      </label>
      <p v-if="archive" class="hint m-0">
        {{ archiveName }} · {{ formatBytes(archive.byteLength) }} ·
        <code>{{ shortDigest(digest) }}</code>
      </p>
    </section>

    <section class="form-section">
      <h3>Manifest</h3>
      <p v-if="manifestSource" class="hint m-0">{{ manifestSource }}</p>
      <label class="grid gap-1 text-xs text-fg-subtle">
        Pick a {{ MANIFEST_FILE }}
        <input type="file" accept=".json,application/json" @change="onManifestFile" />
      </label>
      <CodeEditor v-model="manifestText" language="json" :title="MANIFEST_FILE" />
    </section>

    <section v-if="manifest" class="form-section">
      <h3>Publishing</h3>
      <DataTable table-class="w-full border-collapse text-xs">
        <tbody>
          <tr>
            <td class="border-b border-border-subtle px-2 py-1.5 text-left">Package</td>
            <td class="mono border-b border-border-subtle px-2 py-1.5 text-left">
              {{ qualifiedPackageName(manifest) }}
            </td>
          </tr>
          <tr>
            <td class="border-b border-border-subtle px-2 py-1.5 text-left">Runtime</td>
            <td class="mono border-b border-border-subtle px-2 py-1.5 text-left">
              {{ manifest.runtime.runtime }}
            </td>
          </tr>
          <tr>
            <td class="border-b border-border-subtle px-2 py-1.5 text-left">Exports</td>
            <td class="mono border-b border-border-subtle px-2 py-1.5 text-left">
              {{ exportNames }}
            </td>
          </tr>
        </tbody>
      </DataTable>
      <label class="grid gap-1 text-xs text-fg-subtle">
        Alias to move onto this version
        <input
          v-model.trim="alias"
          maxlength="64"
          pattern="[A-Za-z][A-Za-z0-9_-]*"
          title="Start with a letter and use only letters, numbers, underscores, and hyphens."
          :placeholder="manifest.alias ?? DEFAULT_ALIAS"
        />
        <span class="hint">
          Leave blank to use the manifest's. Calls already compiled keep the version they were built
          against — only new ones follow the alias.
        </span>
      </label>
      <p v-if="aliasError" class="error m-0 text-xs" role="alert">{{ aliasError }}</p>
    </section>

    <p v-if="error" class="error">{{ error }}</p>

    <template #actions>
      <Button variant="ghost" @click="emit('close')">Cancel</Button>
      <Button variant="primary" :loading="busy" :disabled="!canPublish" @click="publish">
        Publish
      </Button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { archiveDigest } from "../../../core/services/functions";
import { useFunctionsStore } from "../../adapters/pinia/functions";
import {
  DEFAULT_ALIAS,
  MANIFEST_FILE,
  parseManifest,
  qualifiedPackageName,
  shortDigest,
} from "../../../core/domain/models";
import type { FunctionManifest } from "../../../core/domain/models";
import { readZipTextEntry } from "../../../core/utils/zip";
import Modal from "../shared/Modal.vue";
import Button from "../shared/Button.vue";
import CodeEditor from "../shared/CodeEditor.vue";

const emit = defineEmits<{ close: []; published: [] }>();

const functions = useFunctionsStore();

const archive = ref<ArrayBuffer | null>(null);
const archiveName = ref("");
const digest = ref("");
const manifestText = ref("");
const manifestSource = ref("");
const alias = ref("");
const error = ref("");
const busy = ref(false);

// the manifest only exists once the text parses; everything that describes the publish reads it, so
// a half-typed manifest simply shows nothing rather than a shape that is not there yet.
const manifest = computed<FunctionManifest | null>(() => {
  if (!manifestText.value.trim()) {
    return null;
  }

  try {
    return parseManifest(manifestText.value);
  } catch {
    return null;
  }
});

const exportNames = computed(() =>
  (manifest.value?.exports ?? []).map((entry) => entry.name).join(", "),
);

const aliasError = computed(() => {
  const value = alias.value.trim();

  if (!value) {
    return "";
  }

  if (!/^[A-Za-z]/.test(value)) {
    return "Alias must start with a letter.";
  }

  return /^[A-Za-z0-9_-]+$/.test(value)
    ? ""
    : "Alias may contain only letters, numbers, underscores, and hyphens.";
});
const canPublish = computed(
  () => Boolean(archive.value && manifest.value) && !busy.value && !aliasError.value,
);

// a manifest that does not parse is worth saying so about, but only once something has been typed.
watch(manifestText, () => {
  error.value = "";

  if (!manifestText.value.trim()) {
    return;
  }

  try {
    parseManifest(manifestText.value);
  } catch (cause) {
    error.value = message(cause);
  }
});

async function onArchive(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0];

  if (!file) {
    return;
  }

  error.value = "";

  if (!file.name.toLowerCase().endsWith(".zip")) {
    error.value = "Choose a .zip package archive.";
    return;
  }

  const bytes = await file.arrayBuffer();
  const signature = new Uint8Array(bytes, 0, Math.min(4, bytes.byteLength));

  if (signature.length < 2 || signature[0] !== 0x50 || signature[1] !== 0x4b) {
    error.value = "The selected file is not a valid ZIP archive.";
    return;
  }

  archive.value = bytes;
  archiveName.value = file.name;
  digest.value = await archiveDigest(bytes);
  await readManifestFromArchive(bytes);
}

// the manifest usually travels inside the archive, so it is read from there rather than asked for
// twice. a zip this reader cannot walk just leaves the field to be filled in by hand.
async function readManifestFromArchive(bytes: ArrayBuffer) {
  const entry = await readZipTextEntry(bytes, (name) => name.split("/").pop() === MANIFEST_FILE);

  if (!entry) {
    manifestSource.value = manifestText.value
      ? ""
      : `No ${MANIFEST_FILE} in the archive — pick or paste one.`;
    return;
  }

  manifestText.value = entry.text;
  manifestSource.value = `Read ${entry.name} from the archive.`;
}

async function onManifestFile(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0];

  if (!file) {
    return;
  }

  manifestText.value = await file.text();
  manifestSource.value = `Read ${file.name}.`;
}

async function publish() {
  if (!archive.value || !manifest.value) {
    return;
  }

  busy.value = true;
  error.value = "";

  try {
    await functions.publish({
      manifest: manifest.value,
      archive: archive.value,
      // an empty box means "whatever the manifest says", which is not the same as "move nothing".
      alias: alias.value.trim() ? alias.value.trim() : undefined,
    });
    emit("published");
    emit("close");
  } catch (cause) {
    error.value = message(cause);
  } finally {
    busy.value = false;
  }
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function formatBytes(value: number): string {
  const units = ["B", "KiB", "MiB", "GiB"];
  let size = value;
  let unit = 0;

  while (size >= 1024 && unit + 1 < units.length) {
    size /= 1024;
    unit += 1;
  }

  return `${size.toFixed(1)} ${units[unit]}`;
}
</script>
