import { onScopeDispose, ref, type Ref } from "vue";
import { createStore } from "../../../core/services/event-bus";

export function bindServiceState<T>(service: ReturnType<typeof createStore<T>>): Ref<T> {
  const state = ref(service.getState()) as Ref<T>;

  const unsubscribe = service.subscribe(() => {
    state.value = service.getState();
  });

  onScopeDispose(unsubscribe);

  return state;
}

export function mirrorServiceState<T>(service: ReturnType<typeof createStore<T>>) {
  const state = ref(service.getState()) as Ref<T>;
  service.subscribe(() => {
    state.value = service.getState();
  });
  return state;
}
