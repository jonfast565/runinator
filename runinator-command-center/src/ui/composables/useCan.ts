import type { Action } from "../../core/domain/models";
import { useActionsStore } from "../adapters/pinia/actions";

// ergonomic action gate for templates and setup code: `const { can } = useCan()` then
// `v-if="can('secrets:write')"` or `:disabled="!can('nodes:scale')"`. `can` reads reactive state, so
// gated markup updates when the caller's actions change (sign-in, org switch).
export function useCan() {
  const store = useActionsStore();
  const can = (action: Action): boolean => store.has(action);
  return { can };
}
