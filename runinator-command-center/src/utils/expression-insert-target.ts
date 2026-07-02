import { ref } from "vue";

// the currently focused expression editor's splice-at-cursor callback, if any. a single shared slot
// lets a dialog-level reference list insert into whichever expression field the user last focused
// without each field having to wire itself to that list.
export type ExpressionInsert = (text: string) => void;

const target = ref<ExpressionInsert | null>(null);

// the reactive target, for callers that branch on whether a field is focused (insert vs. copy).
export function useExpressionInsertTarget() {
  return target;
}

// claim the slot when an expression editor gains focus.
export function setExpressionInsertTarget(insert: ExpressionInsert) {
  target.value = insert;
}

// release the slot on blur, but only if this editor still owns it.
export function clearExpressionInsertTarget(insert: ExpressionInsert) {
  if (target.value === insert) {
    target.value = null;
  }
}
