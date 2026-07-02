import type { RunArtifact } from "../domain/models";
import type { ArtifactUploadRequest } from "../api/commandCenterApi";
import type { CommandRuntime } from "../api/runtime";

export interface AuthStorage {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}

export interface FilePicker {
  pickFile(): Promise<File | null>;
}

export interface ArtifactDownloadResult {
  saved_to: string | null;
}

export interface ArtifactTransport {
  isDesktop(): boolean;
  pickFile(): Promise<File | null>;
  uploadFromPath(request: ArtifactUploadRequest): Promise<RunArtifact>;
  uploadFromBrowser(request: ArtifactUploadRequest, file: File): Promise<RunArtifact>;
  downloadInBrowser(artifactId: string, name: string): Promise<void>;
  downloadToPath(artifactId: string, name: string): Promise<ArtifactDownloadResult>;
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
  artifacts: ArtifactTransport;
  serviceDiscovery: ServiceDiscovery;
  filePicker?: FilePicker;
}
