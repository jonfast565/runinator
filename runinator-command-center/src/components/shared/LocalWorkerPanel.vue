<template>
  <div v-if="store.supported" class="panel local-worker-panel">
    <div class="panel-toolbar">
      <h2>Desktop Worker</h2>
      <span class="replica-stat">{{ store.status.running ? "running" : "stopped" }}</span>
    </div>

    <p class="local-worker-hint">
      Run this machine as a worker so workflows can read and write files in a sandboxed folder on
      your desktop. Actions are routed only to this replica.
    </p>

    <div v-if="store.status.running" class="local-worker-status">
      <div><strong>Replica:</strong> {{ store.status.replica_id }}</div>
      <div><strong>Root:</strong> {{ store.status.root }}</div>
      <div><strong>Broker:</strong> {{ store.status.broker_url }}</div>
      <button class="btn" :disabled="store.busy" @click="store.stop()">
        <Icon name="stop" />
        <span>Stop worker</span>
      </button>
    </div>

    <form v-else class="local-worker-form" @submit.prevent="onStart">
      <label>
        <span>Broker URL</span>
        <input v-model="brokerUrl" type="text" placeholder="http://127.0.0.1:8088/" required />
      </label>
      <label>
        <span>Sandbox folder</span>
        <input v-model="sandboxRoot" type="text" placeholder="/Users/me/runinator-files" required />
      </label>
      <label class="local-worker-check">
        <input v-model="allowWrite" type="checkbox" />
        <span>Allow writes and deletes</span>
      </label>
      <button class="btn" type="submit" :disabled="store.busy">
        <Icon name="play" />
        <span>Start worker</span>
      </button>
    </form>

    <div v-if="store.error" class="empty-state">{{ store.error }}</div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import Icon from "./Icon.vue";
import { useLocalWorkerStore } from "../../stores/localWorker";

const store = useLocalWorkerStore();
const brokerUrl = ref("http://127.0.0.1:8088/");
const sandboxRoot = ref("");
const allowWrite = ref(false);

onMounted(() => {
  void store.refresh();
});

async function onStart() {
  await store.start({
    broker_url: brokerUrl.value,
    sandbox_root: sandboxRoot.value,
    allow_write: allowWrite.value,
  });
}
</script>

<style scoped>
.local-worker-panel {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 1rem;
}
.local-worker-hint {
  margin: 0;
  opacity: 0.75;
  font-size: 0.85rem;
}
.local-worker-form,
.local-worker-status {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.local-worker-form label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.85rem;
}
.local-worker-check {
  flex-direction: row !important;
  align-items: center;
  gap: 0.5rem;
}
</style>
