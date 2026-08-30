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
        ref="mainContent"
        :inert="app.interactionsDisabled"
        :aria-disabled="app.interactionsDisabled"
        @invalid.capture="onInvalid"
        @input.capture="onFormInput"
        @change.capture="onFormInput"
        @focusout.capture="onFormInput"
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
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { useKeyboardShortcuts } from "../../composables/useKeyboardShortcuts";
import Icon from "../shared/Icon.vue";
import OutageBanner from "./OutageBanner.vue";
import SidebarNav from "./SidebarNav.vue";
import ToastHost from "./ToastHost.vue";
import TopToolbar from "./TopToolbar.vue";
import { applyDefaultConstraints, validateFormControl } from "../../composables/form-validation";

const app = useAppStore();
const { handleKeydown, refreshActive } = useKeyboardShortcuts();
const validationMessage = ref("");
const mainContent = ref<HTMLElement | null>(null);
let invalidControl: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement | null = null;
let constraintObserver: MutationObserver | null = null;

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
  validateFormControl(control);
  control.setAttribute("aria-invalid", "true");

  if (!invalidControl || invalidControl.validity.valid) {
    invalidControl = control;
    validationMessage.value = `${fieldLabel(control)}: ${control.validationMessage}`;
  }
}

function onFormInput(event: Event) {
  if (!isFormControl(event.target)) {
    return;
  }

  applyDefaultConstraints(event.target.closest("form") ?? event.target.parentElement ?? document);

  if (!validateFormControl(event.target)) {
    event.target.setAttribute("aria-invalid", "true");
    return;
  }

  event.target.removeAttribute("aria-invalid");

  if (event.target === invalidControl) {
    invalidControl = null;
    validationMessage.value = "";
  }
}

onMounted(() => {
  if (!mainContent.value) {
    return;
  }

  applyDefaultConstraints(mainContent.value);
  constraintObserver = new MutationObserver(() => {
    if (mainContent.value) {
      applyDefaultConstraints(mainContent.value);
    }
  });
  constraintObserver.observe(mainContent.value, { childList: true, subtree: true });
});

onBeforeUnmount(() => {
  constraintObserver?.disconnect();
  constraintObserver = null;
});

function focusInvalidField() {
  invalidControl?.focus();
  invalidControl?.scrollIntoView({ behavior: "smooth", block: "center" });
}

watch(
  () => app.activeTab,
  async () => {
    invalidControl = null;
    validationMessage.value = "";
    await nextTick();

    if (mainContent.value) {
      applyDefaultConstraints(mainContent.value);
    }
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
