import {
  deleteFunctionAlias,
  deleteFunctionPackage,
  fetchFunctionCatalog,
  fetchFunctionPackage,
  fetchFunctionPackages,
  publishFunctionVersion,
  restoreFunctionPackage,
  setFunctionAlias,
  uploadFunctionArtifact,
} from "../api/commandCenterApi";
import type {
  FunctionCatalogEntry,
  FunctionManifest,
  FunctionPackage,
  FunctionPackageDetail,
  FunctionVersion,
} from "../domain/models";
import { publishRequest, qualifiedPackageName, validateManifest } from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";
import type { ConfirmContext } from "./operation-context";

/// what a publish uploads: an already-built archive and the manifest that describes it.
export interface FunctionPublish {
  manifest: FunctionManifest;
  archive: ArrayBuffer;
  /// an alias to move instead of the manifest's, or `null` to move none.
  alias?: string | null;
}

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
    // publishing from an archive the operator built. `runinatorctl functions publish` archives a
    // directory itself, deterministically; a browser has no working tree to archive, so the zip
    // arrives already made and is addressed by the digest of exactly those bytes.
    async publish({ manifest, archive, alias }: FunctionPublish): Promise<FunctionVersion | null> {
      validateManifest(manifest);
      const digest = await archiveDigest(archive);
      const request = publishRequest(manifest, digest);

      if (alias !== undefined) {
        request.alias = alias;
      }

      const published = await app.runOperation(`Publishing ${qualifiedPackageName(manifest)}`, async () => {
        // the server keeps the bytes only if it does not already hold that digest, so republishing
        // unchanged code is a no-op rather than a second copy.
        await uploadFunctionArtifact(digest, archive);
        return publishFunctionVersion(request);
      });
      app.setStatus(
        `Published ${qualifiedPackageName(manifest)} version ${String(published.version)}`,
      );
      await service.refreshPackages();
      return published;
    },
    async restore(packageName: string) {
      await app.runOperation("Restoring function package", () =>
        restoreFunctionPackage(packageName),
      );
      app.setStatus(`Restored ${packageName}`);
      await service.refreshPackages();
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

/// `sha256:<hex>` of the archive, which is the address the platform stores it under.
///
/// computed here rather than taken from the caller: a digest that did not come from these exact
/// bytes would publish a version pinned to something else entirely.
export async function archiveDigest(archive: ArrayBuffer): Promise<string> {
  const hashed = await crypto.subtle.digest("SHA-256", archive);
  const hex = [...new Uint8Array(hashed)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return `sha256:${hex}`;
}
