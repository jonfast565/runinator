import { describe, expect, it } from "vitest";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import type { JsonRecord } from "../../../../types/models";
import KeyValueObjectEditor from "../KeyValueObjectEditor.vue";
import {
  removeObjectKey,
  renameObjectKey,
  setObjectValue,
  uniqueObjectKey,
} from "../../../../utils/key-value-object";

describe("KeyValueObjectEditor", () => {
  it("renders scalar, object, and WDL expression values as editable rows", async () => {
    const app = createSSRApp({
      render: () =>
        h(KeyValueObjectEditor, {
          modelValue: {
            message: "hello",
            retries: 3,
            enabled: true,
            payload: { nested: "value" },
            from_params: { $ref: { params: ["ticket_id"] } },
          },
          title: "Parameters",
        }),
    });

    const html = await renderToString(app);

    expect(html).toContain("Parameters");
    expect(html).toContain("message");
    expect(html).toContain("retries");
    expect(html).toContain("enabled");
    expect(html).toContain("payload");
    expect(html).toContain("from_params");
    expect(html).toContain("expression-editor-shell");
    expect(html).toContain("Value");
    expect(html).toContain("Expression");
  });

  it("adds, renames, updates, and removes rows without losing sibling values", () => {
    let value: JsonRecord = { message: "hello", retries: 1 };
    const key = uniqueObjectKey(value);

    value = setObjectValue(value, key, null);
    expect(value).toEqual({ message: "hello", retries: 1, key: null });

    const renamed = renameObjectKey(value, "key", "enabled");
    expect(renamed.error).toBe("");
    value = setObjectValue(renamed.value, "enabled", true);
    expect(value).toEqual({ message: "hello", retries: 1, enabled: true });

    value = removeObjectKey(value, "retries");
    expect(value).toEqual({ message: "hello", enabled: true });
  });

  it("rejects empty and duplicate keys without changing the object", () => {
    const value = { message: "hello", retries: 1 };

    expect(renameObjectKey(value, "message", "").value).toBe(value);
    expect(renameObjectKey(value, "message", "").error).toBe("Key is required");
    expect(renameObjectKey(value, "message", "retries").value).toBe(value);
    expect(renameObjectKey(value, "message", "retries").error).toBe("Key already exists");
  });
});
