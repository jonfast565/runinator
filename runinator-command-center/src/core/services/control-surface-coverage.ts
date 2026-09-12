import manifestJson from "../navigation/control-surface-coverage.json";
import type { AppTab } from "../navigation/app";

export const coverageRequirements = [
  "discoverable",
  "configurable",
  "observable",
  "controllable",
  "intentional",
] as const;

export type CoverageRequirement = (typeof coverageRequirements)[number];
export type CoverageOutcome = "surface" | "intentional" | "gap";

export interface CoverageAnswer {
  outcome: CoverageOutcome;
  detail: string;
}

export interface CapabilityCoverage {
  id: string;
  area: string;
  label: string;
  summary: string;
  tabs: AppTab[];
  coverage: Record<CoverageRequirement, CoverageAnswer>;
}

export interface ControlSurfaceCoverageManifest {
  version: number;
  requirements: CoverageRequirement[];
  capabilities: CapabilityCoverage[];
}

export const controlSurfaceCoverage = manifestJson as ControlSurfaceCoverageManifest;

export function capabilityHasGap(capability: CapabilityCoverage): boolean {
  return coverageRequirements.some(
    (requirement) => capability.coverage[requirement].outcome === "gap",
  );
}

export function coverageOutcomeLabel(outcome: CoverageOutcome): string {
  switch (outcome) {
    case "surface":
      return "Exposed";
    case "intentional":
      return "Intentional";
    default:
      return "Gap";
  }
}
