export interface AuthStorage {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}

export interface FilePicker {
  pickFile(): Promise<File | null>;
}

export interface PlatformAdapter {
  runtime: import("../api/runtime").CommandRuntime;
  authStorage: AuthStorage;
  filePicker?: FilePicker;
}
