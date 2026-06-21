<template>
  <li class="settings-tree-node">
    <template v-if="node.type === 'folder'">
      <button type="button" class="settings-tree-folder" @click="expanded = !expanded">
        <Icon name="chevron-right" :size="13" class="settings-tree-caret" :class="{ open: expanded }" />
        <Icon name="folder" :size="14" />
        <span class="settings-tree-label">{{ node.label }}</span>
        <span class="settings-tree-count">{{ leafCount }}</span>
      </button>
      <ul v-show="expanded" class="settings-tree-children">
        <SettingsTreeNode
          v-for="child in node.children"
          :key="child.path"
          :node="child"
          :is-config="isConfig"
          :config-values="configValues"
          :selected-key="selectedKey"
          @select="$emit('select', $event)"
        />
      </ul>
    </template>
    <button
      v-else
      type="button"
      class="settings-tree-leaf"
      :class="{ selected: selectedKey === secretKey(node.setting) }"
      @click="$emit('select', node.setting)"
    >
      <Icon name="key" :size="13" class="settings-tree-leaf-icon" />
      <span class="settings-tree-label">{{ node.label }}</span>
      <code class="settings-tree-ref">{{ settingRef(node.setting.kind, node.setting.scope, node.setting.name) }}</code>
      <span v-if="isConfig" class="settings-tree-value">{{ configValues[secretKey(node.setting)] || "—" }}</span>
    </button>
  </li>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import Icon from "./Icon.vue";
import { secretKey, settingRef } from "../../utils/secrets";
import type { CredentialSummary } from "../../types/models";
import type { SettingsTreeNode as TreeNode } from "../../utils/settings-tree";

defineOptions({ name: "SettingsTreeNode" });

const props = defineProps<{
  node: TreeNode;
  isConfig: boolean;
  configValues: Record<string, string>;
  selectedKey: string;
}>();

defineEmits<{ select: [setting: CredentialSummary] }>();

const expanded = ref(true);

// count the settings beneath a folder for the badge.
const leafCount = computed(() => countLeaves(props.node));

function countLeaves(node: TreeNode): number {
  if (node.type === "leaf") return 1;
  return node.children.reduce((total, child) => total + countLeaves(child), 0);
}
</script>

<style scoped>
.settings-tree-node {
  list-style: none;
}

.settings-tree-folder,
.settings-tree-leaf {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: var(--radius);
  background: transparent;
  color: var(--text);
  cursor: pointer;
  font: inherit;
  text-align: left;
}

.settings-tree-folder:hover,
.settings-tree-leaf:hover {
  background: var(--surface-muted);
}

.settings-tree-leaf.selected {
  background: var(--surface-subtle);
  outline: 1px solid var(--border-strong);
}

.settings-tree-caret {
  transition: transform 0.12s ease;
}

.settings-tree-caret.open {
  transform: rotate(90deg);
}

.settings-tree-label {
  font-weight: 600;
}

.settings-tree-leaf .settings-tree-label {
  font-weight: 500;
}

.settings-tree-count {
  margin-left: auto;
  color: var(--text-muted);
  font-size: 11px;
}

.settings-tree-ref {
  color: var(--text-muted);
  font-size: 12px;
}

.settings-tree-leaf .settings-tree-ref {
  margin-left: auto;
}

.settings-tree-value {
  max-width: 280px;
  overflow: hidden;
  color: var(--text-muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.settings-tree-children {
  margin: 0;
  padding: 0 0 0 18px;
  border-left: 1px solid var(--border-subtle);
}
</style>
