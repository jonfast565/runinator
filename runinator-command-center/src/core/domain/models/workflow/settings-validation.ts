import { artifactIdentityErrors } from "../artifact/identity";

export const WORKFLOW_VERSION_PATTERN = "\\d+\\.\\d+\\.\\d+";

export interface WorkflowSettingsIdentity {
  name: string;
  namespace?: string | null;
  key?: string | null;
  version: string;
}

export interface WorkflowSettingsErrors {
  name: string;
  namespace: string;
  key: string;
  version: string;
}

export function workflowSettingsErrors(identity: WorkflowSettingsIdentity): WorkflowSettingsErrors {
  const errors = artifactIdentityErrors(identity);
  const version = identity.version.trim();

  return {
    ...errors,
    version: !version
      ? "Version is required."
      : new RegExp(`^${WORKFLOW_VERSION_PATTERN}$`).test(version)
        ? ""
        : "Use a semantic version with major, minor, and patch numbers, for example 1.0.0.",
  };
}

export function workflowSettingsError(identity: WorkflowSettingsIdentity): string {
  const errors = workflowSettingsErrors(identity);
  return errors.name || errors.namespace || errors.key || errors.version;
}
