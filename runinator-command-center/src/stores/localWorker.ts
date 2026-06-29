import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  type LocalWorkerConfig,
  type LocalWorkerStatus,
  localWorkerStatus,
  startLocalWorker,
  stopLocalWorker,
} from '../api/commandCenterApi'
import { isTauriRuntime } from '../api/tauriRuntime'

// the embedded desktop worker only exists in the tauri runtime; the browser/http build cannot run it.
export const useLocalWorkerStore = defineStore('localWorker', () => {
  const supported = isTauriRuntime()
  const status = ref<LocalWorkerStatus>({
    running: false,
    replica_id: null,
    root: null,
    broker_url: null,
  })
  const busy = ref(false)
  const error = ref<string | null>(null)

  async function refresh() {
    if (!supported) return
    try {
      status.value = await localWorkerStatus()
    } catch (err: any) {
      error.value = err?.message || 'Failed to read local worker status'
    }
  }

  async function start(config: LocalWorkerConfig) {
    if (!supported) return
    busy.value = true
    error.value = null
    try {
      status.value = await startLocalWorker(config)
    } catch (err: any) {
      error.value = err?.message || 'Failed to start local worker'
    } finally {
      busy.value = false
    }
  }

  async function stop() {
    if (!supported) return
    busy.value = true
    error.value = null
    try {
      status.value = await stopLocalWorker()
    } catch (err: any) {
      error.value = err?.message || 'Failed to stop local worker'
    } finally {
      busy.value = false
    }
  }

  return { supported, status, busy, error, refresh, start, stop }
})
