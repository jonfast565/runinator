import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";
import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_WIDTH_STORAGE_KEY,
  clampSidebarWidth,
  parseSidebarWidth,
} from "../../core/navigation/sidebar-width";

const KEYBOARD_STEP = 16;
const KEYBOARD_STEP_LARGE = 48;

/**
 * Drives the sidebar's drag handle: publishes `--sidebar-width` on the document and persists it.
 * `sidebar` is the rail element the pointer position is measured against.
 */
export function useSidebarWidth(sidebar: Ref<HTMLElement | null>) {
  const width = ref(SIDEBAR_DEFAULT_WIDTH);
  const dragging = ref(false);

  // persist:false is for corrections the user did not ask for (restore, viewport clamp), so a
  // narrow window never overwrites the width they chose on a wide one.
  function apply(next: number, persist = true) {
    width.value = next;
    document.documentElement.style.setProperty("--sidebar-width", `${String(next)}px`);

    if (persist) {
      window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(next));
    }
  }

  function setWidth(next: number) {
    apply(clampSidebarWidth(next, window.innerWidth));
  }

  function startDrag(event: PointerEvent) {
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragging.value = true;
    // suppresses the shell's width transition and holds the resize cursor for the whole drag.
    document.documentElement.dataset.sidebarResizing = "true";
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", stopDrag);
  }

  function onPointerMove(event: PointerEvent) {
    const left = sidebar.value?.getBoundingClientRect().left ?? 0;
    setWidth(event.clientX - left);
  }

  function stopDrag() {
    dragging.value = false;
    delete document.documentElement.dataset.sidebarResizing;
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", stopDrag);
  }

  function onKeydown(event: KeyboardEvent) {
    const step = event.shiftKey ? KEYBOARD_STEP_LARGE : KEYBOARD_STEP;

    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setWidth(width.value - step);
    }

    if (event.key === "ArrowRight") {
      event.preventDefault();
      setWidth(width.value + step);
    }

    if (event.key === "Home") {
      event.preventDefault();
      reset();
    }
  }

  /** double-click or Home on the handle returns the rail to its default width. */
  function reset() {
    apply(SIDEBAR_DEFAULT_WIDTH);
  }

  function onViewportResize() {
    apply(clampSidebarWidth(width.value, window.innerWidth), false);
  }

  onMounted(() => {
    apply(
      parseSidebarWidth(window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY), window.innerWidth),
      false,
    );
    window.addEventListener("resize", onViewportResize);
  });

  onBeforeUnmount(() => {
    stopDrag();
    window.removeEventListener("resize", onViewportResize);
  });

  return { width, dragging, startDrag, onKeydown, reset };
}
