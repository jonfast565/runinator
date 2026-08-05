import type { RunOperationOptions, ToastAction } from "./app";

export interface OperationContext {
  runOperation<T>(
    label: string,
    operation: () => Promise<T>,
    options?: RunOperationOptions,
  ): Promise<T>;
  setStatus(text: string): void;
  setError(text: string, action?: ToastAction): void;
  normalizedSearch: string;
}

export interface ConfirmContext {
  confirm(message: string): boolean;
  prompt(message: string): string | null;
}
