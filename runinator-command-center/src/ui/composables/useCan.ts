import type { Capability } from "../../core/domain/models";
import { useCapabilitiesStore } from "../adapters/pinia/capabilities";

/// ergonomic capability gate for templates and setup code: `const { can } = useCan()` then
/// `v-if="can('secrets:write')"` or `:disabled="!can('nodes:scale')"`. `can` reads reactive state, so
/// gated markup updates when the caller's capabilities change (sign-in, org switch).
export function useCan() {
  const store = useCapabilitiesStore();
  const can = (capability: Capability): boolean => store.has(capability);
  return { can };
}
