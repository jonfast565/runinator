import type { JsonValue } from "../json";
import type { WorkflowBundle } from "./workflow/bundle";
import type { WorkflowDefinition } from "./workflow/definition";
import type { WorkflowTrigger } from "./workflow/trigger";
import type { RexRapSettingRef } from "./rexrap/rexrap";

export interface DevPackFile {
  path: string;
  kind: string;
  size_bytes?: number | null;
  modified_at?: string | null;
}

export interface DevPackInspectResult {
  path: string;
  files: DevPackFile[];
  workflows: WorkflowDefinition[];
  triggers: WorkflowTrigger[];
  settings_count: number;
  settings: RexRapSettingRef[];
}

export interface DevPackTextFile {
  path: string;
  content: string;
  modified_at?: string | null;
}

export interface DevPackApplyResult {
  path: string;
  files: DevPackFile[];
  imported: {
    workflows: WorkflowBundle;
    secrets?: {
      secrets?: JsonValue[];
    };
  };
}
