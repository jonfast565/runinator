<template>
  <div
    class="app-shell"
    :class="{
      'sidebar-collapsed': app.sidebarCollapsed,
      'interactions-disabled': app.interactionsDisabled,
      'mobile-nav-open': app.mobileNavOpen,
    }"
    tabindex="0"
    @keydown="onShellKeydown"
  >
    <SidebarNav />
    <div
      v-if="app.mobileNavOpen"
      class="mobile-nav-backdrop"
      aria-hidden="true"
      @click="app.closeMobileNav()"
    ></div>
    <section class="workspace">
      <TopToolbar @refresh="refreshActive" />
      <OutageBanner />
      <main :inert="app.interactionsDisabled" :aria-disabled="app.interactionsDisabled">
        <slot />
      </main>
      <div v-if="app.serviceBlocked" class="app-loader-overlay">
        <div class="app-loader">
          <div class="app-loader-spinner"></div>
          <p>{{ app.loadingMessage }}</p>
        </div>
      </div>
    </section>
    <ToastHost />
  </div>
</template>

<script setup lang="ts">
import { useAppStore } from "../../../stores/app";
import { useKeyboardShortcuts } from "../../composables/useKeyboardShortcuts";
import OutageBanner from "./OutageBanner.vue";
import SidebarNav from "./SidebarNav.vue";
import ToastHost from "./ToastHost.vue";
import TopToolbar from "./TopToolbar.vue";

const app = useAppStore();
const { handleKeydown, refreshActive } = useKeyboardShortcuts();

function onShellKeydown(event: KeyboardEvent) {
  if (app.interactionsDisabled) {
    return;
  }

  if (event.key === "Escape" && app.mobileNavOpen) {
    app.closeMobileNav();
    return;
  }

  handleKeydown(event);
}
</script>
