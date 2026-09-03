import type { ExecutionProfileCommand, ExecutionProfileInput, ExecutionProfileSource } from ".";

export interface ExecutionProfileValidation {
  fields: Record<string, string>;
  valid: boolean;
  summary: string;
}

function relativeBundlePath(value: string): boolean {
  if (!value || value.startsWith("/") || /^[A-Za-z]:/.test(value) || value.includes("\\")) {
    return false;
  }

  return value.split("/").every((part) => part !== "" && part !== "." && part !== "..");
}

function validateCommand(
  command: ExecutionProfileCommand | null | undefined,
  path: string,
  fields: Record<string, string>,
  options: { required?: boolean; interactive?: boolean } = {},
) {
  if (!command) {
    if (options.required) {
      fields[path] = "Add an executable and any arguments.";
    }

    return;
  }

  if (!command.argv.length || command.argv.some((argument) => !argument.trim())) {
    fields[path] = "Every argv entry must contain a value.";
  }

  if (command.interactive && !options.interactive) {
    fields[`${path}.interactive`] = "This command cannot be interactive.";
  }
}

function validateSource(
  source: ExecutionProfileSource,
  index: number,
  fields: Record<string, string>,
) {
  const path = `sources.${String(index)}`;

  if (source.type !== "command" && !source.path.trim()) {
    fields[`${path}.path`] = "Choose a local source path.";
  }

  if (!relativeBundlePath(source.target.trim())) {
    fields[`${path}.target`] =
      "Use a relative bundle path without '.', '..', backslashes, or empty segments.";
  }

  if (source.type === "directory" && !source.glob?.trim()) {
    fields[`${path}.glob`] = "Enter a glob, such as * or **/*.json.";
  }

  if (source.type === "command") {
    validateCommand(source.command, `${path}.command`, fields, { required: true });
  }
}

export function validateExecutionProfile(
  profile: ExecutionProfileInput,
): ExecutionProfileValidation {
  const fields: Record<string, string> = {};
  const name = profile.name.trim();

  if (!name) {
    fields.name = "Name is required.";
  } else if (name.length > 256) {
    fields.name = "Name must be 256 characters or fewer.";
  }

  if (profile.description.length > 16_384) {
    fields.description = "Description must be 16,384 characters or fewer.";
  }

  if (!profile.credential_scopes.length) {
    fields.credential_scopes = "Add at least one credential scope.";
  } else if (profile.credential_scopes.some((scope) => !scope.trim())) {
    fields.credential_scopes = "Credential scopes cannot be blank.";
  } else if (
    new Set(profile.credential_scopes.map((scope) => scope.toLowerCase())).size !==
    profile.credential_scopes.length
  ) {
    fields.credential_scopes = "Credential scopes must be unique.";
  }

  if (profile.collection.version !== 1) {
    fields.collection = "Only collection specification version 1 is supported.";
  }

  if (!profile.collection.sources.length) {
    fields.sources = "Add at least one file, folder, or command source.";
  }

  validateCommand(profile.collection.probe, "probe", fields);
  validateCommand(profile.collection.refresh, "refresh", fields, { interactive: true });
  profile.collection.sources.forEach((source, index) => {
    validateSource(source, index, fields);
  });

  const targets = profile.collection.sources.map((source) => source.target.trim().toLowerCase());
  targets.forEach((target, index) => {
    if (target && targets.indexOf(target) !== index) {
      fields[`sources.${String(index)}.target`] = "Each source needs a unique bundle destination.";
    }
  });

  if (profile.exposure.version !== 1) {
    fields.exposure = "Only exposure specification version 1 is supported.";
  }

  for (const [name, value] of Object.entries(profile.exposure.environment)) {
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      fields[`environment.${name}.name`] = "Use a portable environment-variable name.";
    }

    if (
      value.startsWith("/") ||
      /^[A-Za-z]:/.test(value) ||
      value.includes("../") ||
      value.endsWith("/..") ||
      value.replaceAll("${PROFILE_ROOT}", "").replaceAll("${PROFILE_HOME}", "").includes("${")
    ) {
      fields[`environment.${name}.value`] =
        "Use only ${PROFILE_ROOT} or ${PROFILE_HOME} for paths; traversal and absolute paths are forbidden.";
    }
  }

  const count = Object.keys(fields).length;

  return {
    fields,
    valid: count === 0,
    summary:
      count === 0
        ? "Profile configuration is valid."
        : `${String(count)} field${count === 1 ? " needs" : "s need"} attention.`,
  };
}
