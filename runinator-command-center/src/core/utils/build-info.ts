// the build stamp vite injects at compile time (see `define` in vite.config.ts). it is a constant,
// not a request: the running bundle knows which build it is without asking the web service.

export interface BuildInfo {
  /** package version, kept in step with the rust workspace version. */
  version: string;
  /** commit or release stamp this bundle was built from; empty when neither was available. */
  buildId: string;
  /** iso-8601 instant the bundle was built. */
  builtAt: string;
}

export const buildInfo: BuildInfo = {
  version: __APP_VERSION__,
  buildId: __APP_BUILD_ID__,
  builtAt: __APP_BUILD_TIME__,
};

/** compact one-line stamp for chrome: `v0.3.500 · a1b2c3d`. */
export function versionLabel(info: BuildInfo = buildInfo): string {
  const version = `v${info.version}`;
  return info.buildId ? `${version} · ${info.buildId}` : version;
}

/** multi-line tooltip spelling out every part of the stamp. */
export function buildTooltip(info: BuildInfo = buildInfo): string {
  const lines = [`Command Center v${info.version}`];

  if (info.buildId) {
    lines.push(`Build ${info.buildId}`);
  }

  const built = new Date(info.builtAt);

  if (!Number.isNaN(built.getTime())) {
    lines.push(`Built ${built.toLocaleString()}`);
  }

  return lines.join("\n");
}
