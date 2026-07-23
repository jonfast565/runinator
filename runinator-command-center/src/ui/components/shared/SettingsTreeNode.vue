<template>
  <li class="settings-tree-node">
    <template v-if="node.type === 'folder'">
      <button type="button" class="settings-tree-folder" @click="expanded = !expanded">
        <Icon
          name="chevron-right"
          :size="13"
          class="settings-tree-caret"
          :class="{ open: expanded }"
        />
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
      <code class="settings-tree-ref">{{
        settingRef(node.setting.kind, node.setting.scope, node.setting.name)
      }}</code>
      <span v-if="isConfig" class="settings-tree-value">{{
        configValues[secretKey(node.setting)] || "—"
      }}</span>
    </button>
  </li>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import Icon from "./Icon.vue";
import { secretKey, settingRef } from "../../../core/utils/secrets";
import type { CredentialSummary } from "../../../core/domain/models";
import type { SettingsTreeNode as TreeNode } from "../../../core/utils/settings-tree";

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
  if (node.type === "leaf") {
    return 1;
  }

  return node.children.reduce((total, child) => total + countLeaves(child), 0);
}
</script>

