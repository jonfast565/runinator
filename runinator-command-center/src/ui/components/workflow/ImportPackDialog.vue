<template>
  <Modal title="Import workflow pack" width="min(560px, calc(100vw - 32px))" @close="emit('close')">
    <template #help>
      Choose a compiled pack ZIP. It may contain a <code>.rexrapm</code> manifest plus workflows,
      settings, pipelines, and packaged functions. Source directories and standalone manifests are
      compiled and imported by <code>runinatorctl workflows apply</code> or the desktop Dev Pack
      panel.
    </template>

    <section class="form-section">
      <label class="grid gap-1 text-xs text-fg-subtle">
        Pack ZIP
        <input type="file" accept=".zip,application/zip" @change="chooseFile" />
      </label>
      <p v-if="file" class="hint m-0">{{ file.name }} · {{ formatBytes(file.size) }}</p>
    </section>

    <label class="flex items-start gap-2 text-sm">
      <input v-model="overwrite" type="checkbox" />
      <span>
        Overwrite existing pack-managed items. Leave off to preserve newer definitions already in
        the service.
      </span>
    </label>

    <p v-if="error" class="error">{{ error }}</p>

    <template #actions>
      <Button variant="ghost" @click="emit('close')">Cancel</Button>
      <Button variant="primary" :loading="busy" :disabled="!file" @click="importPack">
        Import pack
      </Button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import Button from "../shared/Button.vue";
import Modal from "../shared/Modal.vue";

const emit = defineEmits<{ close: []; imported: [] }>();
const workflows = useWorkflowsStore();
const file = ref<File | null>(null);
const overwrite = ref(false);
const busy = ref(false);
const error = ref("");

function chooseFile(event: Event) {
  file.value = (event.target as HTMLInputElement).files?.[0] ?? null;
  error.value = "";
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${String(bytes)} B`;
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

async function importPack() {
  if (!file.value) {
    return;
  }

  busy.value = true;
  error.value = "";

  try {
    await workflows.importWorkflowPack(await file.value.arrayBuffer(), overwrite.value);
    emit("imported");
    emit("close");
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    busy.value = false;
  }
}
</script>
