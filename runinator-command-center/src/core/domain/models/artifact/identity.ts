export interface ArtifactIdentity {
  name: string;
  namespace?: string | null;
  key?: string | null;
}

export const REXRAP_IDENTIFIER_PATTERN = "[A-Za-z_][A-Za-z0-9_]*";

const identifier = new RegExp(`^${REXRAP_IDENTIFIER_PATTERN}$`);
const MAX_IDENTITY_LENGTH = 256;

export function artifactIdentityError(identity: ArtifactIdentity): string {
  const name = identity.name.trim();
  const namespace = identity.namespace?.trim() ?? "";
  const key = identity.key?.trim() ?? "";

  if (!name) {
    return "Name is required.";
  }

  if (name.length > MAX_IDENTITY_LENGTH) {
    return `Name must be at most ${String(MAX_IDENTITY_LENGTH)} characters.`;
  }

  if (!namespace) {
    return "Namespace is required; use dot-separated identifiers such as acme.delivery.";
  }

  if (namespace.length > MAX_IDENTITY_LENGTH) {
    return `Namespace must be at most ${String(MAX_IDENTITY_LENGTH)} characters.`;
  }

  if (!namespace.split(".").every((segment) => identifier.test(segment))) {
    return "Each namespace segment must start with a letter or underscore and contain only letters, numbers, or underscores.";
  }

  if (!key) {
    return "Stable key is required.";
  }

  if (key.length > MAX_IDENTITY_LENGTH) {
    return `Stable key must be at most ${String(MAX_IDENTITY_LENGTH)} characters.`;
  }

  if (!identifier.test(key)) {
    return "Stable key must start with a letter or underscore and contain only letters, numbers, or underscores.";
  }

  return "";
}

export function artifactIdentityPath(identity: ArtifactIdentity): string {
  const namespace = identity.namespace?.trim() ?? "";
  const key = identity.key?.trim() ?? "";
  return namespace && key ? `${namespace}.${key}` : key;
}
