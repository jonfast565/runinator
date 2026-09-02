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
          <input
            v-model.number="dwellDurationSeconds"
            class="input"
            type="number"
            min="1"
            max="30"
            step="1"
          />
          <span>s</span>
        </label>
        <Button :loading="loading" @click="scheduleRefresh(true)">
          <Icon name="refresh" />
          <span>Refresh</span>
        </Button>
      </PanelHeader>

      <div class="ingress-tabs" role="tablist" aria-label="Ingress streams">
        <button
          v-for="item in tabs"
          :key="item.id"
          class="ingress-tab"
          :class="{ active: section === item.id }"
          @click="section = item.id"
        >
          {{ item.label }}
        </button>
      </div>

      <template v-if="section === 'external'">
        <div class="control-strip">
          <label
            >Target
            <select v-model="targetKind" class="input">
              <option value="workflow">Workflow</option>
              <option value="pipeline">Pipeline</option>
            </select>
          </label>
          <label class="grow"
            >Resource ID<input
              v-model.trim="targetId"
              class="input w-full font-mono"
              placeholder="Workflow or pipeline UUID"
          /></label>
          <label
            >Gate
            <select v-model="gateMode" class="input">
              <option value="disabled">Disabled</option>
              <option value="paused">Paused · FIFO</option>
              <option value="review">Review · selected order</option>
            </select>
          </label>
          <Button variant="primary" :disabled="!targetId" @click="saveGate">Apply gate</Button>
          <Button :disabled="!targetId || gateMode !== 'paused'" @click="releaseFifo"
            >Release FIFO</Button
          >
        </div>
        <FlowBoard
          stream="external"
          :records="externalRecords"
          :incoming-ids="incomingIds"
          :dwell-seconds="dwellDurationSeconds"
          @select="selected = $event"
          @approve="approveExternal"
          @drop="dropExternal"
        />
      </template>

      <template v-else-if="section === 'broker'">
        <div class="control-strip">
          <label
            >Exact scope
            <select v-model="scopeKind" class="input">
              <option value="platform">Platform</option>
              <option value="organization">Organization</option>
              <option value="team">Team</option>
              <option value="user">User</option>
            </select>
          </label>
          <label v-if="scopeKind !== 'platform'" class="grow"
            >Scope ID<input
              v-model.trim="scopeId"
              class="input w-full font-mono"
              placeholder="Exact scope UUID"
          /></label>
          <label
            >Inspector
            <select v-model="brokerMode" class="input">
              <option value="off">Off</option>
              <option value="observe">Observe</option>
              <option value="hold_orchestration_nudges">Hold orchestration nudges</option>
            </select>
          </label>
          <Button
            variant="primary"
            :disabled="scopeKind !== 'platform' && !scopeId"
            @click="saveBrokerSession"
            >Apply session</Button
          >
        </div>
        <p class="m-0 text-xs text-fg-muted">
          Scope matching is exact. Hold mode observes every matching command but stages only
          <code>orchestration_intent</code> messages.
        </p>
        <FlowBoard
          stream="broker"
          :records="brokerRecords"
          :incoming-ids="incomingIds"
          :dwell-seconds="dwellDurationSeconds"
          @select="selected = $event"
          @approve="approveBroker"
          @drop="dropBroker"
        />
      </template>

      <DeadLettersView v-else embedded />

      <Transition name="drawer">
        <aside
          v-if="selected && section !== 'dead'"
          class="message-drawer"
          role="complementary"
          aria-label="Message inspection drawer"
          aria-live="polite"
        >
          <div class="drawer-heading">
            <div>
              <div class="panel-eyebrow">Message inspector</div>
              <h3 class="m-0">{{ cardTitle(selected) }}</h3>
            </div>
            <Button variant="ghost" size="sm" @click="selected = null">Close</Button>
          </div>
          <div class="drawer-summary">
            <span class="drawer-state" :class="`drawer-state-${selected.state}`">{{
              selected.state
            }}</span>
            <span class="font-mono text-xs text-fg-muted">{{
              String(selected.id).slice(0, 8)
            }}</span>
          </div>
          <div class="detail-grid">
            <div>
              <span>Scope</span><strong>{{ scopeLabel(selected) }}</strong>
            </div>
            <div>
              <span>Received</span><strong>{{ formatDate(String(selected.received_at)) }}</strong>
            </div>
            <div>
              <span>Decision by</span><strong>{{ selected.reviewed_by || "—" }}</strong>
            </div>
            <div>
              <span>Resolved</span
              ><strong>{{
                selected.resolved_at ? formatDate(String(selected.resolved_at)) : "—"
              }}</strong>
            </div>
          </div>
          <details class="drawer-details" open>
            <summary>Message payload</summary>
            <pre>{{ pretty(selected.command ?? selected.event ?? selected) }}</pre>
          </details>
          <details class="drawer-details">
            <summary>Full record</summary>
            <pre>{{ pretty(selected) }}</pre>
          </details>
        </aside>
      </Transition>
    </div>
  </section>
</template>

<script setup lang="ts">
import {
  TransitionGroup,
  defineComponent,
  h,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
  type PropType,
} from "vue";
import Button from "../components/shared/Button.vue";
import Icon from "../components/shared/Icon.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import DeadLettersView from "./DeadLettersView.vue";
import { useAppStore } from "../adapters/pinia/app";
import { useOrgsStore } from "../adapters/pinia/orgs";
import type { JsonRecord } from "../../core/domain/models";
import { formatDate, pretty } from "../../core/utils/format";
import {
  approveBrokerIngress,
  approveExternalIngress,
  configureBrokerIngressSession,
  configureExternalIngressGate,
  dropBrokerIngress,
  dropExternalIngress,
  listBrokerIngressControl,
  listExternalIngressControl,
  releaseExternalIngress,
} from "../../core/api/commandCenterApi";

type Section = "external" | "broker" | "dead";
type ScopeKind = "platform" | "organization" | "team" | "user";
const tabs: { id: Section; label: string }[] = [
  { id: "external", label: "External Events" },
  { id: "broker", label: "Broker Messages" },
  { id: "dead", label: "Dead Letters" },
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
const dwellDurationSeconds = ref(
  Math.min(30, Math.max(1, Number(localStorage.getItem("runinator.ingressDwellSeconds") ?? 5))),
);
let initialized = false;
let refreshTimer = 0;
let pollTimer = 0;

function idOf(record: JsonRecord) {
  return String(record.id);
}

function scalarText(value: unknown, fallback = "") {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean"
    ? String(value)
    : fallback;
}

function addNewAnimations(records: JsonRecord[]) {
  if (!initialized || document.hidden) {
    records.forEach((record) => knownIds.add(idOf(record)));
    return;
  }

  const additions = records.map(idOf).filter((id) => !knownIds.has(id));
  records.forEach((record) => knownIds.add(idOf(record)));

  if (!additions.length || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    return;
  }

  incomingIds.value = new Set([...incomingIds.value, ...additions]);
  window.setTimeout(() => {
    const next = new Set(incomingIds.value);
    additions.forEach((id) => next.delete(id));
    incomingIds.value = next;
  }, 450);
}

async function refresh(resetAnimations = false) {
  if (document.hidden || loading.value) {
    return;
  }

  loading.value = true;

  if (resetAnimations) {
    initialized = false;
  }

  try {
    const externalQuery = new URLSearchParams({ limit: "500" });
    const brokerQuery = new URLSearchParams({ limit: "500", scope_kind: scopeKind.value });

    if (scopeKind.value !== "platform" && scopeId.value) {
      brokerQuery.set("scope_id", scopeId.value);
    }

    const [external, broker] = await Promise.all([
      listExternalIngressControl(externalQuery),
      scopeKind.value === "platform" || scopeId.value
        ? listBrokerIngressControl(brokerQuery)
        : Promise.resolve([]),
    ]);
    addNewAnimations([...external, ...broker]);
    externalRecords.value = external;
    brokerRecords.value = broker;
    initialized = true;
  } catch (error) {
    app.setError(String(error));
  } finally {
    loading.value = false;
  }
}

function scheduleRefresh(reset = false) {
  window.clearTimeout(refreshTimer);
  refreshTimer = window.setTimeout(() => void refresh(reset), reset ? 0 : 180);
}

async function mutate(operation: () => Promise<unknown>) {
  try {
    await operation();
    scheduleRefresh();
  } catch (error) {
    app.setError(String(error));
  }
}

function saveGate() {
  return mutate(() =>
    configureExternalIngressGate(targetKind.value, targetId.value, gateMode.value),
  );
}

function releaseFifo() {
  return mutate(() => releaseExternalIngress(targetKind.value, targetId.value));
}

function saveBrokerSession() {
  return mutate(() =>
    configureBrokerIngressSession(
      { kind: scopeKind.value, id: scopeKind.value === "platform" ? null : scopeId.value },
      brokerMode.value,
    ),
  );
}

function approveExternal(record: JsonRecord) {
  return mutate(() => approveExternalIngress(idOf(record)));
}

function dropExternal(record: JsonRecord) {
  return mutate(() => dropExternalIngress(idOf(record)));
}

function approveBroker(record: JsonRecord) {
  return mutate(() => approveBrokerIngress(idOf(record)));
}

function dropBroker(record: JsonRecord) {
  return mutate(() => dropBrokerIngress(idOf(record)));
}

function cardTitle(record: JsonRecord) {
  const event = record.event as JsonRecord | undefined;
  return scalarText(record.command_kind ?? event?.event_type, "Ingress message");
}

function scopeLabel(record: JsonRecord) {
  const scope = (record.owner_scope ?? record.scope) as JsonRecord | undefined;
  return scope
    ? `${scalarText(scope.kind)}${scope.id ? ` · ${scalarText(scope.id)}` : ""}`
    : "platform";
}

function onVisibility() {
  if (!document.hidden) {
    scheduleRefresh(true);
  }
}

function onIngressChanged() {
  scheduleRefresh();
}

watch([scopeKind, scopeId], () => {
  scheduleRefresh(true);
});
watch(dwellDurationSeconds, (value) => {
  const clamped = Math.min(30, Math.max(1, Math.round(value || 5)));

  if (clamped !== value) {
    dwellDurationSeconds.value = clamped;
  }

  localStorage.setItem("runinator.ingressDwellSeconds", String(clamped));
});
watch(
  () => orgs.activeOrgId,
  (id) => {
    if (scopeKind.value === "organization") {
      scopeId.value = id ?? "";
    }

    scheduleRefresh(true);
  },
);
onMounted(() => {
  scheduleRefresh(true);
  pollTimer = window.setInterval(() => {
    scheduleRefresh();
  }, 1500);
  document.addEventListener("visibilitychange", onVisibility);
  window.addEventListener("runinator:ingress-control-changed", onIngressChanged);
});
onBeforeUnmount(() => {
  window.clearInterval(pollTimer);
  window.clearTimeout(refreshTimer);
  document.removeEventListener("visibilitychange", onVisibility);
  window.removeEventListener("runinator:ingress-control-changed", onIngressChanged);
});

const FlowBoard = defineComponent({
  props: {
    stream: { type: String, required: true },
    records: { type: Array as PropType<JsonRecord[]>, required: true },
    incomingIds: { type: Object as PropType<Set<string>>, required: true },
    dwellSeconds: { type: Number, required: true },
  },
  emits: ["select", "approve", "drop"],
  setup(props, { emit }) {
    const laneDefs = [
      { id: "incoming", label: "Incoming" },
      { id: "held", label: "Held" },
      { id: "applied", label: "Applied" },
      { id: "dropped", label: "Dropped" },
    ];
    // Keep cards in their actual state lane. Previously, a new held or approved message first
    // rendered in Incoming, then was re-created in its final lane after 450ms. Apart from being
    // visually misleading, separate transition groups cannot animate that re-parenting smoothly.
    const lane = (record: JsonRecord) =>
      record.state === "held"
        ? "held"
        : record.state === "dropped"
          ? "dropped"
          : ["approved", "applying", "applied", "failed"].includes(scalarText(record.state))
            ? "applied"
            : "incoming";
    const visible = (id: string) =>
      props.records
        .filter((record) => lane(record) === id)
        .filter((record) => {
          if (id !== "applied" && id !== "dropped") {
            return true;
          }

          const resolved = Date.parse(scalarText(record.resolved_at));
          return Number.isFinite(resolved) && Date.now() - resolved <= props.dwellSeconds * 1000;
        });

    const title = (record: JsonRecord) => {
      const event = record.event as JsonRecord | undefined;
      return scalarText(record.command_kind ?? event?.event_type, "message");
    };

    const source = (record: JsonRecord) => {
      const event = record.event as JsonRecord | undefined;
      return scalarText(event?.source, props.stream);
    };

    const target = (record: JsonRecord) => {
      const value = record.target as JsonRecord | undefined;
      return value
        ? `${scalarText(value.kind)} · ${scalarText(value.id).slice(0, 8)}`
        : "unassigned";
    };

    const scope = (record: JsonRecord) => {
      const value = (record.scope ?? record.owner_scope) as JsonRecord | undefined;
      return value
        ? `${scalarText(value.kind)}${value.id ? ` · ${scalarText(value.id).slice(0, 8)}` : ""}`
        : "platform";
    };

    const identifier = (record: JsonRecord) =>
      scalarText(record.delivery_id ?? record.id).slice(0, 8);
    const fields = (record: JsonRecord) =>
      props.stream === "broker"
        ? [
            { label: "scope", value: scope(record) },
            { label: "delivery", value: identifier(record) },
          ]
        : [
            { label: "source", value: source(record) },
            { label: "target", value: target(record) },
          ];

    const age = (record: JsonRecord) => {
      const seconds = Math.max(
        0,
        Math.floor((Date.now() - Date.parse(scalarText(record.received_at))) / 1000),
      );
      return seconds < 60 ? `${String(seconds)}s` : `${String(Math.floor(seconds / 60))}m`;
    };

    return () =>
      h(
        "div",
        {
          class: ["flow-board", `flow-board-${props.stream}`],
          "aria-label": `${props.stream} ingress flow`,
        },
        laneDefs.map((definition, index) => {
          const records = visible(definition.id);
          const rendered = records.slice(0, 8);
          return h("div", { class: ["flow-lane", `lane-${definition.id}`] }, [
            h("div", { class: "lane-header" }, [
              h("span", { class: "lane-title" }, [
                h("i", { "aria-hidden": "true" }),
                definition.label,
              ]),
              h("strong", String(records.length)),
            ]),
            h("div", { class: "lane-track" }, [
              index < laneDefs.length - 1
                ? h("div", { class: "flow-connector", "aria-hidden": "true" })
                : null,
              h(TransitionGroup, { name: "queue", tag: "div", class: "lane-cards" }, () =>
                rendered.map((record) =>
                  h(
                    "article",
                    {
                      key: String(record.id),
                      class: [
                        "message-card",
                        { "message-card-fresh": props.incomingIds.has(String(record.id)) },
                      ],
                      role: "button",
                      tabindex: 0,
                      "aria-label": `Inspect ${title(record)} message`,
                      onClick: () => {
                        emit("select", record);
                      },
                      onKeydown: (event: KeyboardEvent) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          emit("select", record);
                        }
                      },
                    },
                    [
                      h("div", { class: "card-top" }, [
                        h("span", { class: "object-glyph", "aria-hidden": "true" }, "{}"),
                        h("strong", title(record)),
                        h(
                          "span",
                          { class: `status status-${String(record.state)}` },
                          String(record.state),
                        ),
                      ]),
                      h(
                        "dl",
                        { class: "object-fields" },
                        fields(record).map((field) =>
                          h("div", { class: "object-field" }, [
                            h("dt", field.label),
                            h("dd", field.value),
                          ]),
                        ),
                      ),
                      h("div", { class: "card-meta" }, [
                        h("span", `age ${age(record)}`),
                        h(
                          "span",
                          record.queue_position
                            ? `position ${scalarText(record.queue_position)}`
                            : "",
                        ),
                      ]),
                      definition.id === "held"
                        ? h("div", { class: "card-actions" }, [
                            record.gate_mode === "paused"
                              ? null
                              : h(
                                  "button",
                                  {
                                    class: "mini-action approve",
                                    onClick: (event: Event) => {
                                      event.stopPropagation();
                                      emit("approve", record);
                                    },
                                  },
                                  "Approve",
                                ),
                            h(
                              "button",
                              {
                                class: "mini-action",
                                onClick: (event: Event) => {
                                  event.stopPropagation();
                                  emit("drop", record);
                                },
                              },
                              "Drop",
                            ),
                          ])
                        : null,
                    ],
                  ),
                ),
              ),
              records.length > rendered.length
                ? h(
                    "div",
                    { class: "more-card" },
                    `+${String(records.length - rendered.length)} more`,
                  )
                : null,
            ]),
          ]);
        }),
      );
  },
});
</script>

<style scoped>
.ingress-control .panel {
  overflow-x: hidden;
  overflow-y: auto;
}
.ingress-tabs {
  display: flex;
  gap: 4px;
  border-bottom: 1px solid var(--border);
}
.ingress-tab {
  border: 0;
  border-bottom: 2px solid transparent;
  background: transparent;
  color: var(--fg-muted);
  padding: 9px 13px;
  cursor: pointer;
  font-weight: 650;
  transition:
    color 160ms ease,
    border-color 160ms ease,
    background-color 160ms ease;
}
.ingress-tab:hover {
  background: var(--surface-subtle);
  color: var(--fg);
}
.ingress-tab.active {
  color: var(--fg);
  border-bottom-color: var(--accent);
}
.control-strip {
  display: flex;
  align-items: end;
  flex-wrap: wrap;
  gap: 10px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface-subtle);
}
.control-strip label {
  display: grid;
  gap: 4px;
  color: var(--fg-muted);
  font-size: 11px;
  font-weight: 650;
}
.dwell-control {
  display: flex;
  align-items: center;
  gap: 5px;
  color: var(--fg-muted);
  font-size: 11px;
  white-space: nowrap;
}
.dwell-control input {
  width: 58px;
}
.flow-board {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
  min-width: 0;
}
/* FlowBoard is rendered as a local dynamic component. Its descendants do not receive this SFC's
   scoped attribute, so they must be intentionally styled through :deep. */
:deep(.flow-lane) {
  min-width: 0;
}
:deep(.lane-header) {
  display: flex;
  min-height: 32px;
  align-items: center;
  justify-content: space-between;
  padding: 0 3px 7px;
  color: var(--fg-muted);
  font-size: 10px;
  font-weight: 800;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
:deep(.lane-title) {
  display: inline-flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}
:deep(.lane-title i) {
  width: 7px;
  height: 7px;
  flex: 0 0 7px;
  border-radius: 999px;
  background: var(--accent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 14%, transparent);
}
:deep(.lane-held .lane-title i) {
  background: var(--warning-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--warning-fg) 14%, transparent);
}
:deep(.lane-applied .lane-title i) {
  background: var(--success-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--success-fg) 14%, transparent);
}
:deep(.lane-dropped .lane-title i) {
  background: var(--danger-fg);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--danger-fg) 14%, transparent);
}
:deep(.lane-header strong) {
  display: inline-grid;
  min-width: 23px;
  min-height: 20px;
  place-items: center;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-pill);
  background: var(--surface);
  padding: 0 6px;
  color: var(--fg);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
}
:deep(.lane-track) {
  position: relative;
  min-height: 184px;
  border: 1px solid var(--border-subtle);
  border-radius: 13px;
  padding: 8px;
  background: linear-gradient(
    180deg,
    color-mix(in srgb, var(--surface-subtle) 78%, var(--surface)) 0%,
    var(--surface) 100%
  );
  box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.36);
}
:deep(.flow-board-broker .lane-track) {
  border-color: color-mix(in srgb, var(--accent) 14%, var(--border-subtle));
  background: linear-gradient(
    180deg,
    color-mix(in srgb, var(--accent-soft) 42%, var(--surface)) 0%,
    var(--surface) 68%
  );
}
:deep(.flow-connector) {
  position: absolute;
  z-index: 2;
  top: 23px;
  right: -13px;
  width: 14px;
  height: 2px;
  border-radius: 2px;
  background: var(--border-strong);
}
:deep(.flow-connector::after) {
  position: absolute;
  top: 50%;
  right: -1px;
  width: 5px;
  height: 5px;
  border-top: 2px solid var(--border-strong);
  border-right: 2px solid var(--border-strong);
  content: "";
  transform: translateY(-50%) rotate(45deg);
}
:deep(.lane-cards) {
  display: grid;
  gap: 8px;
}
:deep(.message-card) {
  position: relative;
  min-width: 0;
  cursor: pointer;
  overflow: hidden;
  border: 1px solid var(--border);
  border-left: 3px solid var(--accent);
  border-radius: 9px;
  background: var(--surface);
  padding: 9px;
  box-shadow:
    0 1px 2px rgb(15 23 42 / 0.06),
    0 5px 12px rgb(15 23 42 / 0.035);
  outline: none;
  transition:
    border-color 180ms ease,
    box-shadow 180ms ease,
    transform 180ms ease;
}
:deep(.message-card:hover),
:deep(.message-card:focus-visible) {
  border-color: color-mix(in srgb, var(--accent) 46%, var(--border));
  border-left-color: var(--accent);
  box-shadow:
    0 0 0 3px var(--accent-ring),
    0 9px 18px rgb(15 23 42 / 0.09);
  transform: translateY(-2px);
}
:deep(.lane-held .message-card) {
  border-left-color: var(--warning-fg);
}
:deep(.lane-dropped .message-card) {
  border-left-color: var(--danger-fg);
  opacity: 0.78;
}
:deep(.lane-applied .message-card) {
  border-left-color: var(--success-fg);
}
:deep(.card-top),
:deep(.card-meta),
:deep(.card-actions) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 7px;
}
:deep(.object-glyph) {
  flex: 0 0 auto;
  color: var(--accent-text);
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 750;
}
:deep(.card-top strong) {
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
:deep(.object-fields) {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 5px;
  margin: 8px 0 0;
}
:deep(.object-field) {
  min-width: 0;
  border: 1px solid var(--border-faint);
  border-radius: 6px;
  background: var(--surface-subtle);
  padding: 4px 5px;
}
:deep(.object-field dt) {
  color: var(--fg-faint);
  font-family: var(--font-mono);
  font-size: 8px;
  font-weight: 700;
  letter-spacing: 0.04em;
  line-height: 1.2;
  text-transform: uppercase;
}
:deep(.object-field dd) {
  min-width: 0;
  overflow: hidden;
  margin: 2px 0 0;
  color: var(--fg-subtle);
  font-family: var(--font-mono);
  font-size: 9px;
  font-weight: 650;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}
:deep(.card-meta) {
  margin-top: 5px;
  color: var(--fg-muted);
  font-size: 10px;
}
:deep(.card-meta span) {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
:deep(.status) {
  flex: 0 0 auto;
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
  padding: 2px 6px;
  color: var(--fg-muted);
  font-size: 9px;
  font-weight: 750;
  letter-spacing: 0.03em;
  text-transform: uppercase;
}
:deep(.status-held) {
  background: var(--warning-bg);
  color: var(--warning-fg);
}
:deep(.status-approved),
:deep(.status-applying),
:deep(.status-applied) {
  background: var(--success-bg);
  color: var(--success-fg);
}
:deep(.status-dropped),
:deep(.status-failed) {
  background: var(--danger-bg);
  color: var(--danger-fg);
}
:deep(.card-actions) {
  justify-content: flex-start;
  margin-top: 8px;
}
:deep(.mini-action) {
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--fg);
  padding: 3px 7px;
  font-size: 10px;
  cursor: pointer;
  transition:
    border-color 160ms ease,
    background 160ms ease,
    color 160ms ease;
}
:deep(.mini-action:hover) {
  border-color: var(--border-hover);
  background: var(--surface-subtle);
}
:deep(.mini-action.approve) {
  background: var(--accent);
  color: white;
  border-color: transparent;
}
:deep(.mini-action.approve:hover) {
  background: var(--accent-hover);
}
:deep(.more-card) {
  margin-top: 8px;
  border: 1px dashed var(--border);
  border-radius: 8px;
  padding: 8px;
  color: var(--fg-muted);
  text-align: center;
  font-size: 11px;
}
.message-drawer {
  position: fixed;
  z-index: 30;
  top: 0;
  right: 0;
  width: min(460px, calc(100vw - 24px));
  height: 100dvh;
  overflow-y: auto;
  border-left: 1px solid var(--border);
  background: var(--surface);
  padding: 20px;
  box-shadow: -16px 0 38px rgb(15 23 42 / 0.18);
}
.drawer-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}
.drawer-heading h3 {
  max-width: 330px;
  overflow: hidden;
  font-size: 17px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.drawer-summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: 14px;
  padding: 8px 9px;
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  background: var(--surface-subtle);
}
.drawer-state {
  border-radius: var(--radius-pill);
  background: var(--surface-muted);
  padding: 3px 7px;
  color: var(--fg-muted);
  font-size: 10px;
  font-weight: 750;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
.drawer-state-held {
  background: var(--warning-bg);
  color: var(--warning-fg);
}
.drawer-state-approved,
.drawer-state-applying,
.drawer-state-applied {
  background: var(--success-bg);
  color: var(--success-fg);
}
.drawer-state-dropped,
.drawer-state-failed {
  background: var(--danger-bg);
  color: var(--danger-fg);
}
.detail-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
  margin-top: 10px;
}
.detail-grid div {
  display: grid;
  gap: 2px;
}
.detail-grid span {
  color: var(--fg-muted);
  font-size: 10px;
  text-transform: uppercase;
}
.detail-grid strong {
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
}
.message-drawer .detail-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
  margin-top: 14px;
}
.message-drawer .detail-grid div {
  min-width: 0;
  padding: 7px;
  border: 1px solid var(--border-faint);
  border-radius: 7px;
  background: var(--surface-subtle);
}
.message-drawer .detail-grid strong {
  font-family: var(--font-mono);
  font-size: 10px;
  white-space: nowrap;
}
.drawer-details {
  margin-top: 14px;
  border: 1px solid var(--border-subtle);
  border-radius: 9px;
  background: var(--surface-subtle);
}
.drawer-details summary {
  cursor: pointer;
  padding: 9px 10px;
  color: var(--fg);
  font-size: 12px;
  font-weight: 700;
}
.drawer-details pre {
  max-height: min(38dvh, 360px);
  overflow: auto;
  margin: 0;
  border-top: 1px solid var(--border-subtle);
  background: var(--surface-sunken);
  padding: 11px;
  color: var(--fg-subtle);
  font-size: 11px;
  line-height: 1.5;
}
.drawer-enter-active,
.drawer-leave-active {
  transition:
    transform 260ms cubic-bezier(0.16, 1, 0.3, 1),
    opacity 180ms ease;
}
.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
  transform: translateX(100%);
}
:deep(.message-card-fresh::after) {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  box-shadow: inset 0 0 0 1px var(--accent-pulse);
  content: "";
  pointer-events: none;
  animation: ingress-arrival 700ms cubic-bezier(0.16, 1, 0.3, 1) both;
}
:deep(.queue-enter-active),
:deep(.queue-leave-active),
:deep(.queue-move) {
  transition:
    transform 320ms cubic-bezier(0.16, 1, 0.3, 1),
    opacity 220ms ease,
    max-height 320ms cubic-bezier(0.16, 1, 0.3, 1);
}
:deep(.queue-enter-from) {
  max-height: 0;
  opacity: 0;
  transform: translateY(-10px);
}
:deep(.queue-enter-to) {
  max-height: 220px;
}
:deep(.queue-leave-to) {
  opacity: 0;
  transform: translateY(-6px) scale(0.98);
}
:deep(.queue-leave-active) {
  position: absolute;
  width: calc(100% - 16px);
}
@keyframes ingress-arrival {
  from {
    box-shadow:
      inset 0 0 0 1px var(--accent-pulse),
      0 0 0 0 var(--accent-pulse-soft);
    opacity: 1;
  }
  to {
    box-shadow:
      inset 0 0 0 1px transparent,
      0 0 0 13px transparent;
    opacity: 0;
  }
}
@media (max-width: 980px) {
  .flow-board {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .detail-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  :deep(.flow-connector) {
    display: none;
  }
}
@media (max-width: 620px) {
  .flow-board {
    grid-template-columns: 1fr;
  }
  :deep(.lane-track) {
    min-height: 112px;
  }
  .message-drawer {
    width: 100vw;
  }
  .drawer-heading h3 {
    max-width: calc(100vw - 130px);
  }
}
@media (prefers-reduced-motion: reduce) {
  :deep(.queue-enter-active),
  :deep(.queue-leave-active),
  :deep(.queue-move),
  :deep(.message-card),
  .drawer-enter-active,
  .drawer-leave-active {
    transition: none !important;
  }
  :deep(.message-card-fresh::after) {
    animation: none !important;
  }
}
</style>
