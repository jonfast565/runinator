import type { ActionMetadata } from "./action-metadata";

export interface ProviderRuntimeMetadata {
  credential_scopes: string[];
  contract?: string | null;
  execution_profile?: "unsupported" | "subprocess" | "in_process";
}

export interface ProviderMetadata {
  name: string;
  actions: ActionMetadata[];
  metadata: ProviderRuntimeMetadata;
}
