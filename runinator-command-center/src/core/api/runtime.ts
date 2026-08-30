export interface CommandRuntime {
  isTauri(): boolean;
  invoke<T>(name: string, args?: Record<string, unknown>): Promise<T>;
  wsBaseUrl(): string;
  apiBaseUrl(): string;
}

let activeRuntime: CommandRuntime | null = null;
let unauthorizedHandler: (() => Promise<boolean>) | null = null;

export function setCommandRuntime(runtime: CommandRuntime) {
  activeRuntime = runtime;
}

export function getCommandRuntimeOptional(): CommandRuntime | null {
  return activeRuntime;
}

export function getCommandRuntime(): CommandRuntime {
  if (!activeRuntime) {
    throw new Error(
      "Command runtime has not been configured. Call setCommandRuntime() at bootstrap.",
    );
  }

  return activeRuntime;
}

export function setUnauthorizedHandler(handler: (() => Promise<boolean>) | null) {
  unauthorizedHandler = handler;
}

export function isTauriRuntime() {
  return getCommandRuntimeOptional()?.isTauri() ?? false;
}

async function command<T>(name: string, args?: Record<string, unknown>) {
  assertTransportSafe(args, "args", new WeakSet(), 0);

  try {
    return await getCommandRuntime().invoke<T>(name, args);
  } catch (error) {
    if (
      unauthorizedHandler &&
      name !== "login" &&
      name !== "refresh_session" &&
      name !== "logout" &&
      /(?:401|unauthorized|unauthenticated)/i.test(String(error))
    ) {
      const recovered = await unauthorizedHandler();

      if (recovered) {
        return getCommandRuntime().invoke<T>(name, args);
      }
    }

    throw error;
  }
}

const MAX_TRANSPORT_DEPTH = 100;
const MAX_TRANSPORT_STRING_LENGTH = 16 * 1024 * 1024;

/** Reject values JSON/Tauri transports cannot represent faithfully before any request is sent. */
export function assertTransportSafe(
  value: unknown,
  path = "value",
  ancestors = new WeakSet(),
  depth = 0,
): void {
  if (depth > MAX_TRANSPORT_DEPTH) {
    throw new Error(`${path} is nested more than ${String(MAX_TRANSPORT_DEPTH)} levels deep`);
  }

  if (value === null || value === undefined || typeof value === "boolean") {
    return;
  }

  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new Error(`${path} must be a finite number`);
    }

    return;
  }

  if (typeof value === "string") {
    if (value.length > MAX_TRANSPORT_STRING_LENGTH) {
      throw new Error(`${path} exceeds the ${String(MAX_TRANSPORT_STRING_LENGTH)} character limit`);
    }

    return;
  }

  if (typeof value !== "object") {
    throw new Error(`${path} contains an unsupported ${typeof value} value`);
  }

  if (
    value instanceof ArrayBuffer ||
    ArrayBuffer.isView(value) ||
    (typeof Blob !== "undefined" && value instanceof Blob) ||
    value instanceof Date
  ) {
    if (value instanceof Date && Number.isNaN(value.getTime())) {
      throw new Error(`${path} must be a valid date`);
    }

    return;
  }

  if (ancestors.has(value)) {
    throw new Error(`${path} contains a circular reference`);
  }

  ancestors.add(value);

  if (Array.isArray(value)) {
    value.forEach((entry, index) => {
      assertTransportSafe(entry, `${path}[${String(index)}]`, ancestors, depth + 1);
    });
  } else {
    for (const [key, entry] of Object.entries(value)) {
      assertTransportSafe(entry, `${path}.${key}`, ancestors, depth + 1);
    }
  }

  ancestors.delete(value);
}

export { command };
