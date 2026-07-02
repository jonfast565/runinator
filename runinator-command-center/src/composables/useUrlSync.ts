import { nextTick, onScopeDispose, watch } from "vue";
import { tabs, useAppStore } from "../stores/app";
import { useWorkflowsStore } from "../stores/workflows";
import { formatRoute, parseRoute } from "../utils/url-sync";
import type { AppTab } from "../types/app";

// keeps the URL hash in sync with the active tab and the selected workflow/run so views are
// deep-linkable (#/Runs/<id>, #/Workflows/<id>) and browser back/forward works. a lightweight
// history layer instead of vue-router, which keeps the existing tab-state architecture intact and
// works identically in the tauri desktop shell and the hosted web SPA.
export function useUrlSync() {
  const app = useAppStore();
  const workflows = useWorkflowsStore();

  // guards writes while we are applying state that came *from* the URL (avoids feedback loops).
  let applyingFromUrl = false;
  // deep-linked ids waiting for their list/detail to load.
  let pendingWorkflowId: string | null = null;
  let pendingRunId: string | null = null;

  const isKnownTab = (tab: string) => (tabs as string[]).includes(tab);

  function parseHash(): { tab: AppTab | null; id: string | null } {
    const { tab, id } = parseRoute(window.location.hash, isKnownTab);
    return { tab: tab as AppTab | null, id };
  }

  function currentHash(): string {
    const tab = app.activeTab;
    let id: string | null = null;

    if (tab === "Workflows") {
      id = workflows.selectedWorkflowId ?? null;
    } else if (tab === "Runs") {
      id = workflows.selectedWorkflowRunId ?? null;
    }

    return formatRoute(tab, id);
  }

  function selectWorkflowById(id: string): boolean {
    const workflow = workflows.workflows.find((candidate) => candidate.id === id);

    if (!workflow) {
      return false;
    }

    void workflows.selectWorkflow(workflow);
    return true;
  }

  function selectRunById(id: string, allowFallbackFetch: boolean): boolean {
    const run = workflows.recentWorkflowRuns.find((candidate) => candidate.id === id);

    if (run) {
      void workflows.selectWorkflowRun(run);
      return true;
    }

    if (!allowFallbackFetch) {
      return false;
    }

    // run isn't in the recent list (e.g. an older shared link); select by a minimal summary so the
    // tab system opens it and fetches the real detail.
    void workflows.selectWorkflowRun({
      id,
      status: "",
      created_at: "",
      started_at: null,
      finished_at: null,
    });
    return true;
  }

  function applyFromUrl() {
    const { tab, id } = parseHash();

    if (!tab) {
      return;
    }

    applyingFromUrl = true;
    app.activeTab = tab;
    pendingWorkflowId = null;
    pendingRunId = null;

    if (tab === "Workflows" && id && !selectWorkflowById(id)) {
      pendingWorkflowId = id;
    } else if (tab === "Runs" && id && !selectRunById(id, true)) {
      pendingRunId = id;
    }

    void nextTick(() => (applyingFromUrl = false));
  }

  function writeUrl(replace = false) {
    if (applyingFromUrl) {
      return;
    }

    const hash = currentHash();

    if (hash === window.location.hash) {
      return;
    }

    const url = `${window.location.pathname}${window.location.search}${hash}`;

    if (replace) {
      window.history.replaceState(null, "", url);
    } else {
      window.history.pushState(null, "", url);
    }
  }

  // initial load: honor a deep link if present, otherwise seed the URL with the current tab.
  if (parseHash().tab) {
    applyFromUrl();
  } else {
    writeUrl(true);
  }

  watch(
    () => [app.activeTab, workflows.selectedWorkflowId, workflows.selectedWorkflowRunId],
    () => {
      writeUrl();
    },
  );

  // resolve pending deep-link selections once the backing data arrives.
  watch(
    () => workflows.workflows.length,
    () => {
      if (pendingWorkflowId && selectWorkflowById(pendingWorkflowId)) {
        pendingWorkflowId = null;
      }
    },
  );
  watch(
    () => workflows.recentWorkflowRuns.length,
    () => {
      if (pendingRunId && selectRunById(pendingRunId, false)) {
        pendingRunId = null;
      }
    },
  );

  window.addEventListener("popstate", applyFromUrl);
  onScopeDispose(() => {
    window.removeEventListener("popstate", applyFromUrl);
  });
}
