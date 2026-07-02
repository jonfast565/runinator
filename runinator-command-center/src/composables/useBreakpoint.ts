import { computed, onScopeDispose, ref } from "vue";

// single source of truth for responsive breakpoints. keep these pixel values in sync with the
// `@media` literals in styles/*.css; layout logic in JS and CSS must agree on where mobile begins.
export const BREAKPOINTS = {
  // stack split panes, narrower sidebar.
  tablet: 1180,
  // drawer nav, master-detail toggle, single-column forms.
  mobile: 760,
  // extra-tight toolbars, card tables.
  compact: 480,
} as const;

export type Viewport = "desktop" | "tablet" | "mobile" | "compact";

// shared reactive state so every consumer observes the same matchMedia results (one listener set).
let tabletMq: MediaQueryList | null = null;
let mobileMq: MediaQueryList | null = null;
let compactMq: MediaQueryList | null = null;
const isTablet = ref(false);
const isMobile = ref(false);
const isCompact = ref(false);
let refCount = 0;

function viewportName(): Viewport {
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

// mirror the active viewport onto the document so CSS can key off `[data-viewport]` when a plain
// media query is not enough (e.g. rules that also depend on component-driven state).
function syncDocument() {
  if (typeof document === "undefined") {
    return;
  }

  document.documentElement.dataset.viewport = viewportName();
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

// reactive viewport info. `tablet`/`mobile`/`compact` are cumulative: a compact screen is also
// mobile and tablet, matching the cascade of `max-width` media queries.
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
