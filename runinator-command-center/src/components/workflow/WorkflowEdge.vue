<template>
  <BaseEdge
    :id="id"
    :path="path"
    :marker-start="markerStart"
    :marker-end="markerEnd"
    :interaction-width="interactionWidth"
    :style="style"
  />
  <!-- leader line + anchor dot tie a displaced label back to the edge it defines. -->
  <g v-if="connectorPath" class="workflow-edge-label-connector nodrag nopan" :class="severityClass">
    <path :d="connectorPath" />
    <circle :cx="anchorPoint.x" :cy="anchorPoint.y" r="2.5" />
  </g>
  <EdgeLabelRenderer v-if="labelText">
    <div
      class="workflow-edge-label nodrag nopan"
      :class="[severityClass, { 'is-interactive': interactive, 'is-manual': hasManualOffset, 'is-dragging': dragging }]"
      :style="labelStyle"
      :title="labelTitle"
      @pointerdown="onPointerDown"
      @dblclick.stop.prevent="onReset"
    >{{ labelText }}</div>
  </EdgeLabelRenderer>
</template>

<script setup lang="ts">
import { computed, inject, onBeforeUnmount, ref, type CSSProperties } from "vue";
import {
  BaseEdge,
  EdgeLabelRenderer,
  Position,
  getBezierPath,
  getSmoothStepPath,
  getStraightPath,
  useVueFlow
} from "@vue-flow/core";
import { useWorkflowsStore } from "../../stores/workflows";
import type { WorkflowEditorEdgeData } from "../../types/models";

// vue flow passes the resolved edge geometry to custom edge components.
const props = defineProps<{
  id: string;
  source: string;
  target: string;
  sourceX: number;
  sourceY: number;
  targetX: number;
  targetY: number;
  sourcePosition: Position;
  targetPosition: Position;
  label?: unknown;
  markerStart?: string;
  markerEnd?: string;
  interactionWidth?: number;
  style?: CSSProperties;
  data?: WorkflowEditorEdgeData;
}>();

const workflows = useWorkflowsStore();
const { getNodes, viewport } = useVueFlow();
// the canvas opts in to label dragging; read-only views (run graph) do not provide it.
const interactive = inject<boolean>("workflowEdgeInteractive", false);

const labelPadding = 6;
const maxAutoShift = 140;

const pathParams = computed(() => {
  const style = props.data?.edgeStyle ?? "square";
  const base = {
    sourceX: props.sourceX,
    sourceY: props.sourceY,
    sourcePosition: props.sourcePosition,
    targetX: props.targetX,
    targetY: props.targetY,
    targetPosition: props.targetPosition
  };
  if (style === "bezier") return getBezierPath(base);
  if (style === "straight") {
    return getStraightPath({ sourceX: props.sourceX, sourceY: props.sourceY, targetX: props.targetX, targetY: props.targetY });
  }
  // square renders as a sharp smoothstep path (border radius 0).
  return getSmoothStepPath({ ...base, borderRadius: 0, offset: props.data?.parallelOffset });
});

const path = computed(() => pathParams.value[0]);
const labelText = computed(() => (typeof props.label === "string" ? props.label : ""));

const manualOffset = computed(() => {
  const offset = props.data?.labelOffset;
  if (!offset || (offset.x === 0 && offset.y === 0)) return null;
  return offset;
});
const hasManualOffset = computed(() => manualOffset.value !== null);

const dragging = ref(false);
const dragOffset = ref<{ x: number; y: number } | null>(null);
let dragStartPointer = { x: 0, y: 0 };
let dragStartOffset = { x: 0, y: 0 };
let dragMoved = false;

const labelDimensions = computed(() => ({
  width: Math.min(180, labelText.value.length * 6.5 + 16),
  height: 20
}));

const labelPosition = computed(() => {
  const [, labelX, labelY] = pathParams.value;
  if (dragging.value && dragOffset.value) return { x: labelX + dragOffset.value.x, y: labelY + dragOffset.value.y };
  if (manualOffset.value) return { x: labelX + manualOffset.value.x, y: labelY + manualOffset.value.y };
  return avoidNodes(labelX, labelY);
});

const labelStyle = computed<CSSProperties>(() => ({
  transform: `translate(-50%, -50%) translate(${labelPosition.value.x}px, ${labelPosition.value.y}px)`,
  pointerEvents: interactive ? "all" : "none"
}));

// the natural label anchor on the path; the leader line points back here.
const anchorPoint = computed(() => {
  const [, labelX, labelY] = pathParams.value;
  return { x: labelX, y: labelY };
});

// draw a leader from the anchor to the label box border once the label is
// shifted (auto-avoidance or manual drag) far enough to read as detached.
const connectorPath = computed(() => {
  const anchor = anchorPoint.value;
  const pos = labelPosition.value;
  const dx = pos.x - anchor.x;
  const dy = pos.y - anchor.y;
  const distance = Math.hypot(dx, dy);
  const { width, height } = labelDimensions.value;
  // while the label still overlaps its anchor the link is already obvious.
  const minDistance = Math.min(width, height) / 2 + labelPadding;
  if (distance <= minDistance) return "";
  // stop the line at the label box edge so it visibly meets the label.
  const scale = Math.min(width / 2 / Math.abs(dx || 1e-6), height / 2 / Math.abs(dy || 1e-6));
  const edgeX = pos.x - dx * scale;
  const edgeY = pos.y - dy * scale;
  return `M ${anchor.x},${anchor.y} L ${edgeX},${edgeY}`;
});

const severityClass = computed(() => props.data?.validationSeverity ?? "");
const labelTitle = computed(() => {
  const messages = props.data?.validationMessages ?? [];
  if (messages.length) return messages.join("\n");
  return interactive ? "Drag to reposition, double-click to reset" : "";
});

// nudge the label out of any node it overlaps along the axis of least penetration.
function avoidNodes(startX: number, startY: number): { x: number; y: number } {
  const { width, height } = labelDimensions.value;
  let x = startX;
  let y = startY;
  for (let pass = 0; pass < 8; pass += 1) {
    let moved = false;
    for (const node of getNodes.value) {
      const nodeWidth = node.dimensions?.width || 180;
      const nodeHeight = node.dimensions?.height || 64;
      const centerX = node.computedPosition.x + nodeWidth / 2;
      const centerY = node.computedPosition.y + nodeHeight / 2;
      const overlapX = width / 2 + nodeWidth / 2 + labelPadding - Math.abs(x - centerX);
      const overlapY = height / 2 + nodeHeight / 2 + labelPadding - Math.abs(y - centerY);
      if (overlapX <= 0 || overlapY <= 0) continue;
      if (overlapX < overlapY) x += (x >= centerX ? 1 : -1) * overlapX;
      else y += (y >= centerY ? 1 : -1) * overlapY;
      moved = true;
    }
    if (!moved) break;
  }
  // keep the auto-shift bounded so a buried label never flies off-screen.
  return {
    x: clamp(x, startX - maxAutoShift, startX + maxAutoShift),
    y: clamp(y, startY - maxAutoShift, startY + maxAutoShift)
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function onPointerDown(event: PointerEvent) {
  if (!interactive || event.button !== 0) return;
  event.stopPropagation();
  const [, labelX, labelY] = pathParams.value;
  const current = labelPosition.value;
  dragStartPointer = { x: event.clientX, y: event.clientY };
  dragStartOffset = { x: current.x - labelX, y: current.y - labelY };
  dragOffset.value = { ...dragStartOffset };
  dragMoved = false;
  dragging.value = true;
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
}

function onPointerMove(event: PointerEvent) {
  if (!dragging.value) return;
  const zoom = viewport.value.zoom || 1;
  const dx = (event.clientX - dragStartPointer.x) / zoom;
  const dy = (event.clientY - dragStartPointer.y) / zoom;
  if (Math.abs(dx) + Math.abs(dy) > 1.5) dragMoved = true;
  dragOffset.value = { x: dragStartOffset.x + dx, y: dragStartOffset.y + dy };
}

function onPointerUp() {
  if (!dragging.value) return;
  const offset = dragOffset.value;
  const moved = dragMoved;
  stopDragging();
  // a click without movement should not freeze the auto-placement into a manual offset.
  if (!moved || !offset) return;
  workflows.setEdgeLabelOffset(props.id, { x: Math.round(offset.x), y: Math.round(offset.y) });
}

function onReset() {
  if (!interactive || !hasManualOffset.value) return;
  workflows.setEdgeLabelOffset(props.id, null);
}

function stopDragging() {
  dragging.value = false;
  dragOffset.value = null;
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", onPointerUp);
}

onBeforeUnmount(stopDragging);
</script>

<style scoped>
.workflow-edge-label-connector {
  pointer-events: none;
}

.workflow-edge-label-connector path {
  fill: none;
  stroke: #94a3b8;
  stroke-width: 1;
  stroke-dasharray: 3 3;
  opacity: 0.8;
}

.workflow-edge-label-connector circle {
  fill: #94a3b8;
  opacity: 0.8;
}

.workflow-edge-label-connector.warning path,
.workflow-edge-label-connector.warning circle {
  stroke: #f59e0b;
  fill: #f59e0b;
}

.workflow-edge-label-connector.error path,
.workflow-edge-label-connector.error circle {
  stroke: #dc2626;
  fill: #dc2626;
}

.workflow-edge-label {
  position: absolute;
  padding: 1px 6px;
  border: 1px solid #cbd5e1;
  border-radius: 10px;
  background: #ffffff;
  color: #34495e;
  font-size: 10px;
  line-height: 1.5;
  white-space: nowrap;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.12);
  user-select: none;
}

.workflow-edge-label.is-interactive {
  cursor: grab;
}

.workflow-edge-label.is-manual {
  border-style: dashed;
  border-color: #94a3b8;
}

.workflow-edge-label.is-dragging {
  cursor: grabbing;
  box-shadow: 0 4px 10px rgba(15, 23, 42, 0.24);
}

.workflow-edge-label.warning {
  border-color: #f59e0b;
  color: #92400e;
}

.workflow-edge-label.error {
  border-color: #dc2626;
  color: #b91c1c;
}
</style>
