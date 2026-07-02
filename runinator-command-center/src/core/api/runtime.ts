export interface CommandRuntime {
  isTauri(): boolean;
  invoke<T>(name: string, args?: Record<string, unknown>): Promise<T>;
  wsBaseUrl(): string;
  apiBaseUrl(): string;
}

let activeRuntime: CommandRuntime | null = null;

export function setCommandRuntime(runtime: CommandRuntime) {
  activeRuntime = runtime;
}

export function getCommandRuntimeOptional(): CommandRuntime | null {
  return activeRuntime;
}

export function getCommandRuntime(): CommandRuntime {
  if (!activeRuntime) {
    throw new Error("Command runtime has not been configured. Call setCommandRuntime() at bootstrap.");
  }

  return activeRuntime;
}

export function isTauriRuntime() {
  return getCommandRuntimeOptional()?.isTauri() ?? false;
}

function command<T>(name: string, args?: Record<string, unknown>) {
  return getCommandRuntime().invoke<T>(name, args);
}

export { command };
