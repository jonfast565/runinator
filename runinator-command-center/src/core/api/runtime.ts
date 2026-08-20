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

export { command };
