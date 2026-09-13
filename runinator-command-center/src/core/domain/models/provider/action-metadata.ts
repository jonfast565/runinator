import type { JsonValue } from "../../json";
import type { DeliverySemantics } from "../orchestration/orchestration";
import type { RuninatorType } from "./runinator-type";

export interface ActionParameterMetadata {
  name: string;
  ty: RuninatorType;
  label?: string | null;
  description?: string | null;
  required: boolean;
  default_value?: JsonValue;
  secret: boolean;
  credential_injections?: CredentialInjection[];
}

export type CredentialInjection =
  | { kind: "parameter"; name: string; template?: string }
  | { kind: "environment"; name: string; template?: string }
  | { kind: "arguments"; values: string[] }
  | { kind: "header"; name: string; template?: string };

export type ActionAuthenticationAlternative =
  { kind: "secrets"; parameters: string[] } | { kind: "execution_profile" };

export interface ActionAuthenticationMetadata {
  required: boolean;
  alternatives: ActionAuthenticationAlternative[];
}

export interface ActionResultMetadata {
  name: string;
  ty: RuninatorType;
  label?: string | null;
  description?: string | null;
}

export interface ActionMetadata {
  function_name: string;
  description?: string | null;
  parameters: ActionParameterMetadata[];
  results: ActionResultMetadata[];
  pure?: boolean;
  delivery_semantics?: DeliverySemantics;
  agent?: AgentActionMetadata | null;
  authentication?: ActionAuthenticationMetadata | null;
}

export interface AgentActionMetadata {
  prompt_parameter: string;
  response_text_pointer: string;
}
