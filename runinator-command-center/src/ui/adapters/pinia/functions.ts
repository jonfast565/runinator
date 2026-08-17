import { defineStore } from "pinia";
import { computed } from "vue";
import { functionsService, appService } from "../../../core/services";
import type { FunctionPackage } from "../../../core/domain/models";
import type { FunctionPublish } from "../../../core/services/functions";
import { mirrorServiceState } from "./sync";

export const useFunctionsStore = defineStore("functions", () => {
  const state = mirrorServiceState(functionsService);

  const filteredPackages = computed(() =>
    functionsService.filteredPackages(appService.normalizedSearch),
  );

  return {
    packages: computed(() => state.value.packages),
    catalog: computed(() => state.value.catalog),
    selectedPackage: computed(() => state.value.selectedPackage),
    filteredPackages,
    // the exports of the selected package across every version, which is what a reader tracing a
    // pinned workflow needs — the package's own `exports` only carries the default alias's.
    selectedExports: computed(() => {
      const id = state.value.selectedPackage?.id;
      return id ? functionsService.exportsForPackage(id) : [];
    }),
    refreshPackages: () => functionsService.refreshPackages(),
    selectPackage: (pkg: FunctionPackage | null) => functionsService.selectPackage(pkg),
    clearFunctions: () => {
      functionsService.clearFunctions();
    },
    promote: (alias: string, version: number) => functionsService.promote(alias, version),
    publish: (request: FunctionPublish) => functionsService.publish(request),
    restore: (packageName: string) => functionsService.restore(packageName),
    removeAlias: (alias: string) =>
      functionsService.removeAlias(alias, {
        confirm: (message) => window.confirm(message),
        prompt: (message) => window.prompt(message),
      }),
    removeSelected: () =>
      functionsService.removeSelected({
        confirm: (message) => window.confirm(message),
        prompt: (message) => window.prompt(message),
      }),
  };
});
