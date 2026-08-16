import { beforeEach, describe, expect, it, vi } from "vitest";
import { createFunctionsService } from "../functions";
import { functionCallPath, shortDigest } from "../../domain/models";
import type { FunctionCatalogEntry, FunctionPackage } from "../../domain/models";
import type { AppService } from "../app";

vi.mock("../../api/commandCenterApi", () => ({
  fetchFunctionPackages: vi.fn(),
  fetchFunctionPackage: vi.fn(),
  fetchFunctionCatalog: vi.fn(),
  deleteFunctionPackage: vi.fn(),
  setFunctionAlias: vi.fn(),
  deleteFunctionAlias: vi.fn(),
}));

import {
  fetchFunctionCatalog,
  fetchFunctionPackage,
  fetchFunctionPackages,
  setFunctionAlias,
} from "../../api/commandCenterApi";

const digest = `sha256:${"a".repeat(64)}`;

function pkg(overrides: Partial<FunctionPackage> = {}): FunctionPackage {
  return {
    id: "package-1",
    org_id: null,
    namespace: null,
    name: "image-tools",
    description: "image utilities",
    latest_version: 3,
    created_at: "2026-08-16T00:00:00Z",
    updated_at: "2026-08-16T00:00:00Z",
    ...overrides,
  };
}

function entry(version: number): FunctionCatalogEntry {
  return {
    package_id: "package-1",
    package_name: "image-tools",
    namespace: null,
    version_id: `version-${String(version)}`,
    version,
    export_id: `export-${String(version)}`,
    export_name: "resize",
    artifact_digest: digest,
    aliases: version === 3 ? ["latest"] : [],
  };
}

// a minimal app stub: runOperation passes the call through so rejections reach the service.
const messages: string[] = [];
const app = {
  runOperation: <T>(_label: string, run: () => Promise<T>) => run(),
  setError: (message: string) => {
    messages.push(message);
  },
  setStatus: (message: string) => {
    messages.push(message);
  },
} as unknown as AppService;

describe("functions service", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(fetchFunctionPackages).mockResolvedValue([pkg()]);
    vi.mocked(fetchFunctionCatalog).mockResolvedValue([entry(1), entry(3), entry(2)]);
    vi.mocked(fetchFunctionPackage).mockResolvedValue({ ...pkg(), versions: [], aliases: [] });
  });

  it("lists a package's exports newest version first", async () => {
    // a reader tracing which version a workflow pinned needs the older ones too, so this reads the
    // catalog rather than the package's `exports` — which only carries the default alias's.
    const service = createFunctionsService(app);
    await service.refreshPackages();

    const versions = service.exportsForPackage("package-1").map((item) => item.version);
    expect(versions).toEqual([3, 2, 1]);
  });

  it("keeps the selection on the same package across a refresh", async () => {
    // publishing should not move what the reader is looking at.
    const service = createFunctionsService(app);
    await service.refreshPackages();
    expect(service.getState().selectedPackage?.id).toBe("package-1");

    vi.mocked(fetchFunctionPackages).mockResolvedValue([pkg({ id: "package-2", name: "other" }), pkg()]);
    vi.mocked(fetchFunctionPackage).mockResolvedValue({ ...pkg(), versions: [], aliases: [] });
    await service.refreshPackages();

    expect(service.getState().selectedPackage?.id).toBe("package-1");
  });

  it("promotes an alias to a named version", async () => {
    const service = createFunctionsService(app);
    await service.refreshPackages();
    await service.promote("production", 2);

    // the package is addressed by its qualified name, which is what the endpoint path expects.
    expect(vi.mocked(setFunctionAlias)).toHaveBeenCalledWith("image-tools", "production", 2);
  });

  it("filters packages by qualified name", async () => {
    const service = createFunctionsService(app);
    vi.mocked(fetchFunctionPackages).mockResolvedValue([
      pkg(),
      pkg({ id: "package-2", namespace: "media", name: "video-tools", description: null }),
    ]);
    await service.refreshPackages();

    expect(service.filteredPackages("media").map((item) => item.id)).toEqual(["package-2"]);
    expect(service.filteredPackages("image").map((item) => item.id)).toEqual(["package-1"]);
  });
});

describe("function display helpers", () => {
  it("renders the dotted call an author writes", () => {
    expect(functionCallPath(entry(3))).toBe("functions.image-tools.resize");
    expect(functionCallPath({ ...entry(3), namespace: "media" })).toBe(
      "functions.media.image-tools.resize",
    );
  });

  it("shortens a digest to something a table can show", () => {
    // a full sha-256 is 71 characters and tells a reader nothing the first few do not.
    expect(shortDigest(digest)).toBe("aaaaaaaaaaaa");
    expect(shortDigest(digest).length).toBe(12);
  });
});
