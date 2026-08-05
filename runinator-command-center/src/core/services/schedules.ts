import {
  createFreezeWindow,
  deleteFreezeWindow,
  fetchFreezeWindows,
  updateFreezeWindow,
} from "../api/commandCenterApi";
import type { FreezeWindow, NewFreezeWindow } from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";

export interface SchedulesState {
  freezeWindows: FreezeWindow[];
  activeOnly: boolean;
}

export function createSchedulesService(app: AppService) {
  const store = createStore<SchedulesState>({
    freezeWindows: [],
    activeOnly: false,
  });

  /// a window covering right now, which is what makes a quiet schedule explainable.
  function activeCount(): number {
    const now = Date.now();

    return store
      .getState()
      .freezeWindows.filter(
        (window) =>
          window.enabled &&
          Date.parse(window.starts_at) <= now &&
          Date.parse(window.ends_at) > now,
      ).length;
  }

  const service = {
    ...store,
    activeCount,
    setActiveOnly(value: boolean) {
      store.setState((state) => ({ ...state, activeOnly: value }));
    },
    async refreshFreezeWindows() {
      const { activeOnly } = store.getState();
      const freezeWindows = await app
        .runOperation("Loading freeze windows", () => fetchFreezeWindows(activeOnly), {
          retryable: true,
        })
        .catch(() => []);
      store.setState((state) => ({ ...state, freezeWindows }));
    },
    async saveFreezeWindow(window: NewFreezeWindow, windowId?: string) {
      // an inverted or empty range is rejected by the backend; surfacing that is the whole value
      // here, since a window that silently freezes nothing is only discovered during the freeze.
      try {
        await app.runOperation("Saving freeze window", () =>
          windowId ? updateFreezeWindow(windowId, window) : createFreezeWindow(window),
        );
      } catch (error: unknown) {
        app.setError(String(error));

        return false;
      }

      await service.refreshFreezeWindows();

      return true;
    },
    async removeFreezeWindow(windowId: string) {
      try {
        await app.runOperation("Deleting freeze window", () => deleteFreezeWindow(windowId));
      } catch (error: unknown) {
        app.setError(String(error));

        return false;
      }

      await service.refreshFreezeWindows();

      return true;
    },
  };

  return service;
}

export type SchedulesService = ReturnType<typeof createSchedulesService>;
