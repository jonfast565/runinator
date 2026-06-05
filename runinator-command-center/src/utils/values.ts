// shared emptiness check for required-field gates. a value counts as blank when
// it is null/undefined, an empty array, or a string that is empty or only
// whitespace. whitespace-only strings must not satisfy a required field.
export function isBlankValue(value: unknown): boolean {
  if (value === undefined || value === null) return true;
  if (typeof value === "string") return value.trim() === "";
  if (Array.isArray(value)) return value.length === 0;
  return false;
}
