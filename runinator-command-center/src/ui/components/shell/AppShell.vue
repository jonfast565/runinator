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
    <Transition name="mobile-nav-backdrop">
      <div
        v-if="app.mobileNavOpen"
        class="mobile-nav-backdrop"
        aria-hidden="true"
        @click="app.closeMobileNav()"
      ></div>
    </Transition>
    <section class="workspace">
      <TopToolbar @refresh="refreshActive" />
      <OutageBanner />
      <Transition name="ui-fade">
        <div v-if="validationMessage" class="form-validation-summary" role="alert">
          <Icon name="alert" :size="14" />
          <span>{{ validationMessage }}</span>
          <button type="button" @click="focusInvalidField">Review field</button>
        </div>
      </Transition>
      <main
        :inert="app.interactionsDisabled"
        :aria-disabled="app.interactionsDisabled"
        @invalid.capture="onInvalid"
        @input.capture="onFormInput"
        @change.capture="onFormInput"
      >
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
import { ref, watch } from "vue";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { useKeyboardShortcuts } from "../../composables/useKeyboardShortcuts";
import Icon from "../shared/Icon.vue";
import OutageBanner from "./OutageBanner.vue";
import SidebarNav from "./SidebarNav.vue";
import ToastHost from "./ToastHost.vue";
import TopToolbar from "./TopToolbar.vue";

const app = useAppStore();
const { handleKeydown, refreshActive } = useKeyboardShortcuts();
const validationMessage = ref("");
let invalidControl: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement | null = null;

function isFormControl(
  target: EventTarget | null,
): target is HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLSelectElement ||
    target instanceof HTMLTextAreaElement
  );
}

function fieldLabel(control: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement): string {
  const explicitLabel = control.id
    ? document.querySelector<HTMLLabelElement>(`label[for="${CSS.escape(control.id)}"]`)
    : null;
  const wrappingLabel = control.closest("label");
  const label = explicitLabel ?? wrappingLabel;
  const namedPart = label?.querySelector("span")?.textContent.trim() ?? label?.textContent.trim();
  const placeholder = control instanceof HTMLSelectElement ? "" : control.placeholder;
  const candidates = [
    control.getAttribute("aria-label"),
    namedPart?.split("\n")[0]?.trim(),
    placeholder,
    control.name,
  ];

  for (const candidate of candidates) {
    if (candidate) {
      return candidate;
    }
  }

  return "this field";
}

function onInvalid(event: Event) {
  if (!isFormControl(event.target)) {
    return;
  }

  const control = event.target;
  control.setAttribute("aria-invalid", "true");

  if (!invalidControl || invalidControl.validity.valid) {
    invalidControl = control;
    validationMessage.value = `${fieldLabel(control)}: ${control.validationMessage}`;
  }
}

function onFormInput(event: Event) {
  if (!isFormControl(event.target) || !event.target.validity.valid) {
    return;
  }

  event.target.removeAttribute("aria-invalid");

  if (event.target === invalidControl) {
    invalidControl = null;
    validationMessage.value = "";
  }
}

function focusInvalidField() {
  invalidControl?.focus();
  invalidControl?.scrollIntoView({ behavior: "smooth", block: "center" });
}

watch(
  () => app.activeTab,
  () => {
    invalidControl = null;
    validationMessage.value = "";
  },
);

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
