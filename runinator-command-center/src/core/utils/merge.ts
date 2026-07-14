// merge incoming rows into an existing list by identity key so unchanged rows keep their object
// reference across refreshes. lets keyed lists (v-for, DataTable) diff a refresh as an in-place
// update instead of a wholesale replace, avoiding row-level re-render churn. order follows
// `incoming`; rows absent from `incoming` are dropped, and matched rows are patched in place.
export function mergeById<T>(
  current: readonly T[],
  incoming: readonly T[],
  key: (row: T) => string | number = (row) => (row as { id: string | number }).id,
): T[] {
  const byKey = new Map<string | number, T>();

  for (const row of current) {
    byKey.set(key(row), row);
  }

  return incoming.map((next) => {
    const existing = byKey.get(key(next));

    if (!existing || existing === next) {
      return next;
    }

    // reuse the existing object so its identity is stable, patching in the new field values.
    Object.assign(existing as object, next);
    return existing;
  });
}
