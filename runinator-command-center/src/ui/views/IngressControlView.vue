<template>
  <section class="pane ingress-control">
    <div class="panel flex min-h-0 flex-col gap-3">
      <PanelHeader
        title="Ingress Control"
        icon="flag"
        eyebrow="Platform operations"
        description="Observe traffic in flight, hold it at a durable boundary, and release or drop individual messages without changing the delivery path."
      >
        <label class="dwell-control">
          <span>Completed dwell</span>
          <input v-model.number="dwellDurationSeconds" class="input" type="number" min="1" max="30" step="1" />
          <span>s</span>
        </label>
        <Button :loading="loading" @click="scheduleRefresh(true)">
          <Icon name="refresh" />
          <span>Refresh</span>
        </Button>
      </PanelHeader>

      <div class="ingress-tabs" role="tablist" aria-label="Ingress streams">
        <button v-for="item in tabs" :key="item.id" class="ingress-tab" :class="{ active: section === item.id }" @click="section = item.id">
          {{ item.label }}
        </button>
      </div>

      <template v-if="section === 'external'">
        <div class="control-strip">
          <label>Target
            <select v-model="targetKind" class="input"><option value="workflow">Workflow</option><option value="pipeline">Pipeline</option></select>
          </label>
          <label class="grow">Resource ID<input v-model.trim="targetId" class="input w-full font-mono" placeholder="Workflow or pipeline UUID" /></label>
          <label>Gate
            <select v-model="gateMode" class="input"><option value="disabled">Disabled</option><option value="paused">Paused · FIFO</option><option value="review">Review · selected order</option></select>
          </label>
          <Button variant="primary" :disabled="!targetId" @click="saveGate">Apply gate</Button>
          <Button :disabled="!targetId || gateMode !== 'paused'" @click="releaseFifo">Release FIFO</Button>
        </div>
        <FlowBoard stream="external" :records="externalRecords" :incoming-ids="incomingIds" :dwell-seconds="dwellDurationSeconds" @select="selected = $event" @approve="approveExternal" @drop="dropExternal" />
      </template>

      <template v-else-if="section === 'broker'">
        <div class="control-strip">
          <label>Exact scope
            <select v-model="scopeKind" class="input">
              <option value="platform">Platform</option><option value="organization">Organization</option><option value="team">Team</option><option value="user">User</option>
            </select>
          </label>
          <label v-if="scopeKind !== 'platform'" class="grow">Scope ID<input v-model.trim="scopeId" class="input w-full font-mono" placeholder="Exact scope UUID" /></label>
          <label>Inspector
            <select v-model="brokerMode" class="input"><option value="off">Off</option><option value="observe">Observe</option><option value="hold_orchestration_nudges">Hold orchestration nudges</option></select>
          </label>
          <Button variant="primary" :disabled="scopeKind !== 'platform' && !scopeId" @click="saveBrokerSession">Apply session</Button>
        </div>
        <p class="m-0 text-xs text-fg-muted">Scope matching is exact. Hold mode observes every matching command but stages only <code>orchestration_intent</code> messages.</p>
        <FlowBoard stream="broker" :records="brokerRecords" :incoming-ids="incomingIds" :dwell-seconds="dwellDurationSeconds" @select="selected = $event" @approve="approveBroker" @drop="dropBroker" />
      </template>

      <DeadLettersView v-else embedded />

      <aside v-if="selected && section !== 'dead'" class="message-detail" aria-live="polite">
        <div class="flex items-start justify-between gap-3">
          <div><div class="panel-eyebrow">Selected message</div><h3 class="m-0">{{ cardTitle(selected) }}</h3></div>
          <Button variant="ghost" size="sm" @click="selected = null">Close</Button>
        </div>
        <div class="detail-grid">
          <div><span>Scope</span><strong>{{ scopeLabel(selected) }}</strong></div>
          <div><span>Status</span><strong>{{ selected.state }}</strong></div>
          <div><span>Received</span><strong>{{ formatDate(String(selected.received_at)) }}</strong></div>
          <div><span>Decision by</span><strong>{{ selected.reviewed_by || "—" }}</strong></div>
        </div>
        <pre>{{ pretty(selected.command ?? selected.event ?? selected) }}</pre>
      </aside>
    </div>
  </section>
</template>

<script setup lang="ts">
import { TransitionGroup, defineComponent, h, onBeforeUnmount, onMounted, ref, watch, type PropType } from "vue";
import Button from "../components/shared/Button.vue";
import Icon from "../components/shared/Icon.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import DeadLettersView from "./DeadLettersView.vue";
import { useAppStore } from "../adapters/pinia/app";
import { useOrgsStore } from "../adapters/pinia/orgs";
import type { JsonRecord } from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";
import {
  approveBrokerIngress, approveExternalIngress, configureBrokerIngressSession,
  configureExternalIngressGate, dropBrokerIngress, dropExternalIngress,
  listBrokerIngressControl, listExternalIngressControl, releaseExternalIngress,
} from "../../core/api/commandCenterApi";

type Section = "external" | "broker" | "dead";
type ScopeKind = "platform" | "organization" | "team" | "user";
const tabs: { id: Section; label: string }[] = [
  { id: "external", label: "External Events" }, { id: "broker", label: "Broker Messages" }, { id: "dead", label: "Dead Letters" },
];
const app = useAppStore();
const orgs = useOrgsStore();
const section = ref<Section>("external");
const loading = ref(false);
const externalRecords = ref<JsonRecord[]>([]);
const brokerRecords = ref<JsonRecord[]>([]);
const selected = ref<JsonRecord | null>(null);
const incomingIds = ref(new Set<string>());
const knownIds = new Set<string>();
const targetKind = ref<"workflow" | "pipeline">("workflow");
const targetId = ref("");
const gateMode = ref<"disabled" | "paused" | "review">("disabled");
const scopeKind = ref<ScopeKind>(orgs.activeOrgId ? "organization" : "platform");
const scopeId = ref(orgs.activeOrgId ?? "");
const brokerMode = ref<"off" | "observe" | "hold_orchestration_nudges">("off");
const dwellDurationSeconds = ref(Math.min(30, Math.max(1, Number(localStorage.getItem("runinator.ingressDwellSeconds") ?? 5))));
let initialized = false;
let refreshTimer = 0;
let pollTimer = 0;

function idOf(record: JsonRecord) { return String(record.id); }

function scalarText(value: unknown, fallback = "") {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean"
    ? String(value)
    : fallback;
}

function addNewAnimations(records: JsonRecord[]) {
  if (!initialized || document.hidden) { records.forEach((record) => knownIds.add(idOf(record))); return; }
  const additions = records.map(idOf).filter((id) => !knownIds.has(id));
  records.forEach((record) => knownIds.add(idOf(record)));
  if (!additions.length || matchMedia("(prefers-reduced-motion: reduce)").matches) {return;}
  incomingIds.value = new Set([...incomingIds.value, ...additions]);
  window.setTimeout(() => { const next = new Set(incomingIds.value); additions.forEach((id) => next.delete(id)); incomingIds.value = next; }, 450);
}

async function refresh(resetAnimations = false) {
  if (document.hidden || loading.value) {return;}
  loading.value = true;
  if (resetAnimations) {initialized = false;}

  try {
    const externalQuery = new URLSearchParams({ limit: "500" });
    const brokerQuery = new URLSearchParams({ limit: "500", scope_kind: scopeKind.value });
    if (scopeKind.value !== "platform" && scopeId.value) {brokerQuery.set("scope_id", scopeId.value);}
    const [external, broker] = await Promise.all([
      listExternalIngressControl(externalQuery),
      scopeKind.value === "platform" || scopeId.value ? listBrokerIngressControl(brokerQuery) : Promise.resolve([]),
    ]);
    addNewAnimations([...external, ...broker]);
    externalRecords.value = external;
    brokerRecords.value = broker;
    initialized = true;
  } catch (error) { app.setError(String(error)); } finally { loading.value = false; }
}

function scheduleRefresh(reset = false) {
  window.clearTimeout(refreshTimer);
  refreshTimer = window.setTimeout(() => void refresh(reset), reset ? 0 : 180);
}

async function mutate(operation: () => Promise<unknown>) { try { await operation(); scheduleRefresh(); } catch (error) { app.setError(String(error)); } }

function saveGate() { return mutate(() => configureExternalIngressGate(targetKind.value, targetId.value, gateMode.value)); }

function releaseFifo() { return mutate(() => releaseExternalIngress(targetKind.value, targetId.value)); }

function saveBrokerSession() { return mutate(() => configureBrokerIngressSession({ kind: scopeKind.value, id: scopeKind.value === "platform" ? null : scopeId.value }, brokerMode.value)); }

function approveExternal(record: JsonRecord) { return mutate(() => approveExternalIngress(idOf(record))); }

function dropExternal(record: JsonRecord) { return mutate(() => dropExternalIngress(idOf(record))); }

function approveBroker(record: JsonRecord) { return mutate(() => approveBrokerIngress(idOf(record))); }

function dropBroker(record: JsonRecord) { return mutate(() => dropBrokerIngress(idOf(record))); }

function cardTitle(record: JsonRecord) { const event = record.event as JsonRecord | undefined; return scalarText(record.command_kind ?? event?.event_type, "Ingress message"); }

function scopeLabel(record: JsonRecord) { const scope = (record.owner_scope ?? record.scope) as JsonRecord | undefined; return scope ? `${scalarText(scope.kind)}${scope.id ? ` · ${scalarText(scope.id)}` : ""}` : "platform"; }

function onVisibility() { if (!document.hidden) {scheduleRefresh(true);} }

function onIngressChanged() { scheduleRefresh(); }

watch([scopeKind, scopeId], () => { scheduleRefresh(true); });
watch(dwellDurationSeconds, (value) => {
  const clamped = Math.min(30, Math.max(1, Math.round(value || 5)));
  if (clamped !== value) {dwellDurationSeconds.value = clamped;}
  localStorage.setItem("runinator.ingressDwellSeconds", String(clamped));
});
watch(() => orgs.activeOrgId, (id) => { if (scopeKind.value === "organization") {scopeId.value = id ?? "";} scheduleRefresh(true); });
onMounted(() => { scheduleRefresh(true); pollTimer = window.setInterval(() => { scheduleRefresh(); }, 1500); document.addEventListener("visibilitychange", onVisibility); window.addEventListener("runinator:ingress-control-changed", onIngressChanged); });
onBeforeUnmount(() => { window.clearInterval(pollTimer); window.clearTimeout(refreshTimer); document.removeEventListener("visibilitychange", onVisibility); window.removeEventListener("runinator:ingress-control-changed", onIngressChanged); });

const FlowBoard = defineComponent({
  props: { stream: { type: String, required: true }, records: { type: Array as PropType<JsonRecord[]>, required: true }, incomingIds: { type: Object as PropType<Set<string>>, required: true }, dwellSeconds: { type: Number, required: true } },
  emits: ["select", "approve", "drop"],
  setup(props, { emit }) {
    const laneDefs = [
      { id: "incoming", label: "Incoming" }, { id: "held", label: "Held" }, { id: "applied", label: "Applied" }, { id: "dropped", label: "Dropped" },
    ];
    const lane = (record: JsonRecord) => props.incomingIds.has(String(record.id)) ? "incoming" : record.state === "held" ? "held" : record.state === "dropped" ? "dropped" : record.state === "applied" || record.state === "failed" ? "applied" : "incoming";
    const visible = (id: string) => props.records.filter((record) => lane(record) === id).filter((record) => {
      if (id !== "applied" && id !== "dropped") {return true;}
      const resolved = Date.parse(scalarText(record.resolved_at));
      return Number.isFinite(resolved) && Date.now() - resolved <= props.dwellSeconds * 1000;
    });
    const title = (record: JsonRecord) => { const event = record.event as JsonRecord | undefined; return scalarText(record.command_kind ?? event?.event_type, "message"); };
    const source = (record: JsonRecord) => { const event = record.event as JsonRecord | undefined; return scalarText(event?.source, props.stream); };
    const target = (record: JsonRecord) => { const value = record.target as JsonRecord | undefined; return value ? `${scalarText(value.kind)} · ${scalarText(value.id).slice(0, 8)}` : "broker ingress"; };
    const age = (record: JsonRecord) => { const seconds = Math.max(0, Math.floor((Date.now() - Date.parse(scalarText(record.received_at))) / 1000)); return seconds < 60 ? `${String(seconds)}s` : `${String(Math.floor(seconds / 60))}m`; };
    return () => h("div", { class: "flow-board" }, laneDefs.map((definition, index) => {
      const records = visible(definition.id); const rendered = records.slice(0, 8);
      return h("div", { class: ["flow-lane", `lane-${definition.id}`] }, [
        h("div", { class: "lane-header" }, [h("span", definition.label), h("strong", String(records.length))]),
        h("div", { class: "lane-track" }, [
          h("div", { class: "flow-arrow", "aria-hidden": "true" }, index < laneDefs.length - 1 ? "→" : ""),
          h(TransitionGroup, { name: "queue", tag: "div", class: "lane-cards" }, () => rendered.map((record) => h("article", { key: String(record.id), class: "message-card", onClick: () => { emit("select", record); } }, [
            h("div", { class: "card-top" }, [h("strong", title(record)), h("span", { class: `status status-${String(record.state)}` }, String(record.state))]),
            h("div", { class: "card-meta" }, [h("span", source(record)), h("span", target(record))]),
            h("div", { class: "card-meta" }, [h("span", `age ${age(record)}`), h("span", record.queue_position ? `#${scalarText(record.queue_position)}` : "")]),
            definition.id === "held" ? h("div", { class: "card-actions" }, [
              record.gate_mode === "paused" ? null : h("button", { class: "mini-action approve", onClick: (event: Event) => { event.stopPropagation(); emit("approve", record); } }, "Approve"),
              h("button", { class: "mini-action", onClick: (event: Event) => { event.stopPropagation(); emit("drop", record); } }, "Drop"),
            ]) : null,
          ]))),
          records.length > rendered.length ? h("div", { class: "more-card" }, `+${String(records.length - rendered.length)} more`) : null,
        ]),
      ]);
    }));
  },
});
</script>

<style scoped>
.ingress-control .panel { overflow: auto; }
.ingress-tabs { display: flex; gap: 4px; border-bottom: 1px solid var(--border); }
.ingress-tab { border: 0; border-bottom: 2px solid transparent; background: transparent; color: var(--fg-muted); padding: 9px 13px; cursor: pointer; font-weight: 650; }
.ingress-tab.active { color: var(--fg); border-bottom-color: var(--accent); }
.control-strip { display: flex; align-items: end; flex-wrap: wrap; gap: 10px; padding: 10px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-subtle); }
.control-strip label { display: grid; gap: 4px; color: var(--fg-muted); font-size: 11px; font-weight: 650; }
.dwell-control { display: flex; align-items: center; gap: 5px; color: var(--fg-muted); font-size: 11px; white-space: nowrap; }
.dwell-control input { width: 58px; }
.flow-board { display: grid; grid-template-columns: repeat(4, minmax(190px, 1fr)); gap: 10px; min-width: 820px; }
.flow-lane { min-width: 0; }
.lane-header { display: flex; justify-content: space-between; align-items: center; padding: 7px 9px; color: var(--fg-muted); font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: .04em; }
.lane-header strong { min-width: 24px; border-radius: 999px; background: var(--surface-subtle); padding: 2px 7px; text-align: center; color: var(--fg); }
.lane-track { position: relative; min-height: 290px; border: 1px solid var(--border); border-radius: 12px; padding: 8px; background: color-mix(in srgb, var(--surface-sunken) 70%, transparent); }
.flow-arrow { position: absolute; right: -16px; top: 46%; z-index: 2; color: var(--fg-muted); font-size: 20px; }
.lane-cards { display: grid; gap: 8px; }
.message-card { position: relative; cursor: pointer; border: 1px solid var(--border); border-left: 3px solid var(--accent); border-radius: 9px; background: var(--surface); padding: 9px; box-shadow: 0 4px 12px rgb(0 0 0 / .08); }
.lane-held .message-card { border-left-color: var(--warning, #c98b2d); }
.lane-dropped .message-card { border-left-color: var(--danger, #c84b4b); opacity: .82; }
.lane-applied .message-card { border-left-color: var(--success, #3d9b70); }
.card-top, .card-meta, .card-actions { display: flex; align-items: center; justify-content: space-between; gap: 7px; }
.card-top strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
.card-meta { margin-top: 5px; color: var(--fg-muted); font-size: 10px; }
.card-meta span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.status { border-radius: 999px; padding: 2px 6px; background: var(--surface-subtle); color: var(--fg-muted); font-size: 9px; text-transform: uppercase; }
.card-actions { justify-content: flex-start; margin-top: 8px; }
.mini-action { border: 1px solid var(--border); border-radius: 6px; background: transparent; color: var(--fg); padding: 3px 7px; font-size: 10px; cursor: pointer; }
.mini-action.approve { background: var(--accent); color: white; border-color: transparent; }
.more-card { margin-top: 8px; border: 1px dashed var(--border); border-radius: 8px; padding: 8px; color: var(--fg-muted); text-align: center; font-size: 11px; }
.message-detail { border: 1px solid var(--border); border-radius: 12px; background: var(--surface); padding: 12px; }
.detail-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; margin-top: 10px; }
.detail-grid div { display: grid; gap: 2px; }.detail-grid span { color: var(--fg-muted); font-size: 10px; text-transform: uppercase; }.detail-grid strong { overflow: hidden; text-overflow: ellipsis; font-size: 12px; }
.message-detail pre { max-height: 260px; overflow: auto; margin: 10px 0 0; border-radius: 8px; background: var(--surface-sunken); padding: 10px; font-size: 11px; }
.queue-enter-active, .queue-leave-active, .queue-move { transition: transform 280ms ease, opacity 220ms ease; }
.queue-enter-from { transform: translateX(-18px) scale(.98); opacity: 0; }.queue-leave-to { transform: translateY(-8px) scale(.98); opacity: 0; }.queue-leave-active { position: absolute; width: calc(100% - 16px); }
@media (max-width: 980px) { .ingress-control { overflow-x: auto; }.detail-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (prefers-reduced-motion: reduce) { .queue-enter-active, .queue-leave-active, .queue-move { transition: none !important; }.message-card { animation: none !important; } }
</style>
