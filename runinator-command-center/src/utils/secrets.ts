import type { CredentialSummary } from "../types/models";

export const SECRET_REF_PREFIX = "secret://";

export function secretKey(secret: CredentialSummary): string {
  return `${secret.scope}:${secret.name}`;
}

export function secretRef(scope: string, name: string): string {
  return `${SECRET_REF_PREFIX}${encodeURIComponent(scope)}/${encodeURIComponent(name)}`;
}

export function parseSecretRef(value: unknown): CredentialSummary | null {
  if (typeof value !== "string" || !value.startsWith(SECRET_REF_PREFIX)) return null;
  const path = value.slice(SECRET_REF_PREFIX.length);
  const [rawScope, rawName] = path.split("/", 2);
  if (!rawScope || !rawName) return null;
  return {
    scope: decodeURIComponent(rawScope),
    name: decodeURIComponent(rawName)
  };
}

export function secretRefLabel(value: unknown): string {
  const parsed = parseSecretRef(value);
  return parsed ? `${parsed.scope}/${parsed.name}` : "";
}
