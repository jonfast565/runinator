export interface ArtifactIdentity {
  name: string;
  namespace?: string | null;
  key?: string | null;
}

export interface ArtifactIdentityErrors {
  name: string;
  namespace: string;
  key: string;
}

export const REXRAP_IDENTIFIER_PATTERN = "[A-Za-z_][A-Za-z0-9_]*";

const identifier = new RegExp(`^${REXRAP_IDENTIFIER_PATTERN}$`);
const MAX_IDENTITY_LENGTH = 256;

export function artifactIdentityErrors(identity: ArtifactIdentity): ArtifactIdentityErrors {
  const name = identity.name.trim();
  const namespace = identity.namespace?.trim() ?? "";
  const key = identity.key?.trim() ?? "";

  return {
    name: !name
      ? "Name is required."
      : name.length > MAX_IDENTITY_LENGTH
        ? `Name must be at most ${String(MAX_IDENTITY_LENGTH)} characters.`
        : "",
    namespace: !namespace
      ? "Namespace is required; use dot-separated identifiers such as acme.delivery."
      : namespace.length > MAX_IDENTITY_LENGTH
        ? `Namespace must be at most ${String(MAX_IDENTITY_LENGTH)} characters.`
        : !namespace.split(".").every((segment) => identifier.test(segment))
          ? "Each namespace segment must start with a letter or underscore and contain only letters, numbers, or underscores."
          : "",
    key: !key
      ? "Stable key is required."
      : key.length > MAX_IDENTITY_LENGTH
        ? `Stable key must be at most ${String(MAX_IDENTITY_LENGTH)} characters.`
        : !identifier.test(key)
          ? "Stable key must start with a letter or underscore and contain only letters, numbers, or underscores."
          : "",
  };
}

export function artifactIdentityError(identity: ArtifactIdentity): string {
  const errors = artifactIdentityErrors(identity);
  return errors.name || errors.namespace || errors.key;
}

export function artifactIdentityPath(identity: ArtifactIdentity): string {
  const namespace = identity.namespace?.trim() ?? "";
  const key = identity.key?.trim() ?? "";
  return namespace && key ? `${namespace}.${key}` : key;
}
