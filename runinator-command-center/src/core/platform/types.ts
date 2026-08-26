import type { CommandRuntime } from "../api/runtime";

export interface AuthStorage {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}

export interface FilePicker {
  pickFile(): Promise<File | null>;
}

export interface PlatformDialogs {
  confirm(message: string): boolean;
  prompt(message: string): string | null;
}

export interface ServiceStatusSnapshot {
  service_url: string | null;
}

export interface ServiceDiscovery {
  isDesktop(): boolean;
  webServiceUrl(): string;
  getInitialStatus(): Promise<ServiceStatusSnapshot>;
  startDiscovery(): Promise<void>;
  listenServiceUrlChanged(handler: (url: string | null) => void): Promise<() => void>;
  listenDiscoveryError(handler: (message: string) => void): Promise<() => void>;
}

export interface PlatformAdapter {
  runtime: CommandRuntime;
  authStorage: AuthStorage;
  dialogs: PlatformDialogs;
  serviceDiscovery: ServiceDiscovery;
  filePicker?: FilePicker;
}
