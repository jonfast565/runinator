import { onBeforeUnmount, onMounted, type Ref } from "vue";

/**
 * Keep a visible graph fitted when the pane that owns it changes size.
 *
 * Split panes, the sidebar, responsive layouts, and restored views all ultimately resize the graph
 * container. Observing that boundary keeps graph components independent of every layout control
 * that can affect them. Hidden views report a zero-sized box and are ignored until restored.
 */
export function useGraphAutoFit(
  container: Ref<HTMLElement | null>,
  fitGraph: () => unknown,
) {
  let observer: ResizeObserver | undefined;
  let animationFrame = 0;

  function scheduleFit() {
    if (animationFrame) {
      return;
    }

    animationFrame = window.requestAnimationFrame(() => {
      animationFrame = 0;
      const element = container.value;

      if (!element || element.clientWidth <= 0 || element.clientHeight <= 0) {
        return;
      }

      void fitGraph();
    });
  }

  onMounted(() => {
    observer = new ResizeObserver((entries) => {
      const entry = entries.find((candidate) => candidate.target === container.value);

      if (!entry || entry.contentRect.width <= 0 || entry.contentRect.height <= 0) {
        return;
      }

      scheduleFit();
    });

    if (container.value) {
      observer.observe(container.value);
    }
  });

  onBeforeUnmount(() => {
    observer?.disconnect();

    if (animationFrame) {
      window.cancelAnimationFrame(animationFrame);
    }
  });

  return { scheduleFit };
}
