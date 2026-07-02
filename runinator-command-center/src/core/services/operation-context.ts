export interface OperationContext {
  runOperation<T>(label: string, operation: () => Promise<T>): Promise<T>;
  setStatus(text: string): void;
  setError(text: string): void;
  normalizedSearch: string;
}

export interface ConfirmContext {
  confirm(message: string): boolean;
  prompt(message: string): string | null;
}
