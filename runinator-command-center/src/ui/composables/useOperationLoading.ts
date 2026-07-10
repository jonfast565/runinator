import { computed, type ComputedRef } from "vue";
import { useAppStore } from "../adapters/pinia/app";

function matchesLabel(opLabel: string, labels: string[], prefix = false): boolean {
  return labels.some((label) =>
    prefix ? opLabel.startsWith(label) : opLabel === label || opLabel.startsWith(label),
  );
}

export function useOperationLoading(
  labels: string | string[],
  options?: { prefix?: boolean },
): { isLoading: ComputedRef<boolean>; loadingMessage: ComputedRef<string> } {
  const app = useAppStore();
  const labelList = Array.isArray(labels) ? labels : [labels];

  const isLoading = computed(() => {
    if (!app.loading || !app.opLabel) {
      return false;
    }

    return matchesLabel(app.opLabel, labelList, options?.prefix);
  });

  const loadingMessage = computed(() => (isLoading.value ? `${app.opLabel}…` : ""));

  return { isLoading, loadingMessage };
}

export function useAppLoading(): {
  isLoading: ComputedRef<boolean>;
  loadingMessage: ComputedRef<string>;
} {
  const app = useAppStore();

  return {
    isLoading: computed(() => app.loading),
    loadingMessage: computed(() =>
      app.loading && app.opLabel ? `${app.opLabel}…` : app.loading ? "Working…" : "",
    ),
  };
}
