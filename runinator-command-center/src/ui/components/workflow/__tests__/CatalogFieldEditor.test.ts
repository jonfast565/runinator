import { describe, expect, it, vi } from "vitest";
import { createPinia } from "pinia";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import type { NodeFieldMetadata } from "../../../../core/domain/models";
import CatalogFieldEditor from "../CatalogFieldEditor.vue";

vi.mock("../../../../core/services", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/services")>()),
  rexrapLanguageService: {
    renderProgram: vi.fn().mockResolvedValue("compute {\n    return 1\n}"),
  },
}));

describe("CatalogFieldEditor", () => {
  it("uses the REXRAP program preview for retained invocation source", async () => {
    const field: NodeFieldMetadata = {
      name: "program",
      label: "Program",
      description: null,
      required: false,
      secret: false,
      ty: { type: "any" },
      widget: "rexrap_program",
      location: { base: "parameters", path: ["source"] },
    };
    const app = createSSRApp({
      render: () =>
        h(CatalogFieldEditor, {
          field,
          modelValue: [{ $return: 1 }],
        }),
    });
    app.use(createPinia());

    const html = await renderToString(app);

    expect(html).toContain("Rendering program");
    expect(html).not.toContain("any-editor");
    expect(html).not.toContain("json-editor-shell");
  });
});
