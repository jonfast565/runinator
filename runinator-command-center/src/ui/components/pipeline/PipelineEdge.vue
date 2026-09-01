<template>
  <BaseEdge
    :id="id"
    :path="path"
    :marker-start="markerStart"
    :marker-end="markerEnd"
    :interaction-width="interactionWidth"
    :style="style"
  />
  <EdgeLabelRenderer v-if="labelText">
    <div class="graph-edge-label pipeline-edge-label nodrag nopan" :style="labelStyle">
      {{ labelText }}
    </div>
  </EdgeLabelRenderer>
</template>

<script setup lang="ts">
import { computed, type CSSProperties } from "vue";
import { BaseEdge, EdgeLabelRenderer, Position, getBezierPath } from "@vue-flow/core";

const props = defineProps<{
  id: string;
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
}>();

const pathParams = computed(() =>
  getBezierPath({
    sourceX: props.sourceX,
    sourceY: props.sourceY,
    sourcePosition: props.sourcePosition,
    targetX: props.targetX,
    targetY: props.targetY,
    targetPosition: props.targetPosition,
  }),
);
const path = computed(() => pathParams.value[0]);
const labelText = computed(() => (typeof props.label === "string" ? props.label : ""));
const labelStyle = computed<CSSProperties>(() => ({
  transform: `translate(-50%, -50%) translate(${String(pathParams.value[1])}px, ${String(pathParams.value[2])}px)`,
  pointerEvents: "none",
}));
</script>
