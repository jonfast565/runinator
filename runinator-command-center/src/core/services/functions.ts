import {
  deleteFunctionAlias,
  deleteFunctionPackage,
  fetchFunctionCatalog,
  fetchFunctionPackage,
  fetchFunctionPackages,
  setFunctionAlias,
} from "../api/commandCenterApi";
import type {
  FunctionCatalogEntry,
  FunctionPackage,
  FunctionPackageDetail,
} from "../domain/models";
import { qualifiedPackageName } from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";
import type { ConfirmContext } from "./operation-context";

export interface FunctionsState {
  packages: FunctionPackage[];
  selectedPackage: FunctionPackageDetail | null;
  // every published export of every version, which is what a workflow author browses.
  catalog: FunctionCatalogEntry[];
}

export function createFunctionsService(app: AppService) {
  const store = createStore<FunctionsState>({
    packages: [],
    selectedPackage: null,
    catalog: [],
  });

  function filteredPackages(query: string): FunctionPackage[] {
    const packages = store.getState().packages;

    if (!query) {
      return packages;
    }

    return packages.filter((pkg) => {
      const haystack = [qualifiedPackageName(pkg), pkg.description ?? ""]
        .join(" ")
        .toLowerCase();
      return haystack.includes(query);
    });
  }

  // the exports of one package, newest version first. read from the catalog rather than from the
  // selected package's `exports`, which only ever holds the default alias's — a reader looking at
  // version history needs to see what the older versions offered too.
  function exportsForPackage(packageId: string): FunctionCatalogEntry[] {
    return store
      .getState()
      .catalog.filter((entry) => entry.package_id === packageId)
      .sort((left, right) => right.version - left.version);
  }

  const service = {
    ...store,
    filteredPackages,
    exportsForPackage,
    async refreshPackages() {
      const [packages, catalog] = await Promise.all([
        app.runOperation("Refreshing functions", fetchFunctionPackages).catch(() => []),
        app.runOperation("Refreshing function catalog", fetchFunctionCatalog).catch(() => []),
      ]);
      const selectedId = store.getState().selectedPackage?.id;
      store.setState((state) => ({ ...state, packages, catalog }));

      // keep the selection pointed at the same package across a refresh, so publishing does not
      // move what the reader is looking at.
      const stillThere = packages.find((pkg) => pkg.id === selectedId) ?? packages.at(0);

      if (stillThere) {
        await service.selectPackage(stillThere);
        return;
      }

      store.setState((state) => ({ ...state, selectedPackage: null }));
    },
    async selectPackage(pkg: FunctionPackage | null) {
      if (!pkg) {
        store.setState((state) => ({ ...state, selectedPackage: null }));
        return;
      }

      const detail = await app
        .runOperation("Loading function package", () =>
          fetchFunctionPackage(qualifiedPackageName(pkg)),
        )
        .catch(() => null);
      store.setState((state) => ({ ...state, selectedPackage: detail }));
    },
    clearFunctions() {
      store.setState(() => ({ packages: [], selectedPackage: null, catalog: [] }));
    },
    // promotion is the one mutable act on a published package: it changes what *new* calls resolve
    // to and nothing else, because a compiled workflow recorded the version it was built against.
    async promote(alias: string, version: number) {
      const pkg = store.getState().selectedPackage;

      if (!pkg) {
        app.setError("No function package selected");
        return;
      }

      const response = await app.runOperation(`Moving ${alias}`, () =>
        setFunctionAlias(qualifiedPackageName(pkg), alias, version),
      );
      app.setStatus(`Alias ${alias} now points at version ${String(version)}`);
      await service.refreshPackages();
      return response;
    },
    async removeAlias(alias: string, confirm: ConfirmContext) {
      const pkg = store.getState().selectedPackage;

      if (!pkg) {
        app.setError("No function package selected");
        return;
      }

      if (!confirm.confirm(`Delete the alias "${alias}"? The version it names is untouched.`)) {
        return;
      }

      await app
        .runOperation("Deleting alias", () => deleteFunctionAlias(qualifiedPackageName(pkg), alias))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      await service.refreshPackages();
    },
    async removeSelected(confirm: ConfirmContext) {
      const pkg = store.getState().selectedPackage;

      if (!pkg) {
        app.setError("No function package selected");
        return;
      }

      const name = qualifiedPackageName(pkg);

      // spelled out because it is not recoverable and it takes every version with it.
      if (
        !confirm.confirm(
          `Delete "${name}" and every version, export, and alias under it? Workflows already bound to it will stop running.`,
        )
      ) {
        return;
      }

      await app
        .runOperation("Deleting function package", () => deleteFunctionPackage(name))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      await service.refreshPackages();
    },
  };

  return service;
}

export type FunctionsService = ReturnType<typeof createFunctionsService>;
