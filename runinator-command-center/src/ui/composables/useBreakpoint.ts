import { computed, onScopeDispose, ref } from "vue";
import {
  BREAKPOINTS,
  breakpointCustomProperties,
  type ViewportMode,
} from "../../core/navigation/breakpoints";

export { BREAKPOINTS };
export type Viewport = ViewportMode;

let tabletMq: MediaQueryList | null = null;
let mobileMq: MediaQueryList | null = null;
let compactMq: MediaQueryList | null = null;
const isTablet = ref(false);
const isMobile = ref(false);
const isCompact = ref(false);
let refCount = 0;

function viewportName(): ViewportMode {
  if (isCompact.value) {
    return "compact";
  }

  if (isMobile.value) {
    return "mobile";
  }

  if (isTablet.value) {
    return "tablet";
  }

  return "desktop";
}

function syncDocument() {
  if (typeof document === "undefined") {
    return;
  }

  document.documentElement.dataset.viewport = viewportName();

  for (const [name, value] of Object.entries(breakpointCustomProperties())) {
    document.documentElement.style.setProperty(name, value);
  }
}

function update() {
  isTablet.value = tabletMq?.matches ?? false;
  isMobile.value = mobileMq?.matches ?? false;
  isCompact.value = compactMq?.matches ?? false;
  syncDocument();
}

function ensureListeners() {
  if (tabletMq || typeof window === "undefined") {
    return;
  }

  tabletMq = window.matchMedia(`(max-width: ${String(BREAKPOINTS.tablet)}px)`);
  mobileMq = window.matchMedia(`(max-width: ${String(BREAKPOINTS.mobile)}px)`);
  compactMq = window.matchMedia(`(max-width: ${String(BREAKPOINTS.compact)}px)`);
  tabletMq.addEventListener("change", update);
  mobileMq.addEventListener("change", update);
  compactMq.addEventListener("change", update);
  update();
}

function teardownListeners() {
  tabletMq?.removeEventListener("change", update);
  mobileMq?.removeEventListener("change", update);
  compactMq?.removeEventListener("change", update);
  tabletMq = null;
  mobileMq = null;
  compactMq = null;
}

export function useBreakpoint() {
  ensureListeners();
  refCount += 1;

  onScopeDispose(() => {
    refCount -= 1;

    if (refCount <= 0) {
      teardownListeners();
      refCount = 0;
    }
  });

  return {
    isTablet: computed(() => isTablet.value),
    isMobile: computed(() => isMobile.value),
    isCompact: computed(() => isCompact.value),
    viewport: computed(viewportName),
  };
}
