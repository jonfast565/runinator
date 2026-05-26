import { describe, expect, it } from "vitest";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import { createPinia } from "pinia";
import TypedParameterEditor from "../TypedParameterEditor.vue";
import type { ActionParameterMetadata } from "../../../types/models";

describe("TypedParameterEditor", () => {
  it("renders nested struct, map, and union workflow input controls without JSON fallback", async () => {
    const app = createSSRApp({
      render: () => h(TypedParameterEditor, {
        modelValue: {
          workflow_input: {
            target: "prod",
            environments: {
              prod: { url: "https://example.test", retries: 2 }
            },
            strategy: { manual: true }
          }
        },
        parameters: [nestedWorkflowInputParameter()]
      })
    });
    app.use(createPinia());

    const html = await renderToString(app);

    expect(html).toContain("workflow_input");
    expect(html).toContain("environments");
    expect(html).toContain("strategy");
    expect(html).toContain("Add Entry");
    expect(html).toContain("manual");
    expect(html).not.toContain("json-editor-shell");
  });
});

function nestedWorkflowInputParameter(): ActionParameterMetadata {
  return {
    name: "workflow_input",
    label: "Workflow Input",
    description: null,
    required: true,
    secret: false,
    ty: {
      type: "struct",
      fields: {
        target: { required: true, ty: { type: "string" } },
        environments: {
          required: true,
          ty: {
            type: "map",
            values: {
              type: "struct",
              fields: {
                url: { required: true, ty: { type: "string" } },
                retries: { required: false, ty: { type: "integer" } }
              }
            }
          }
        },
        strategy: {
          required: true,
          ty: {
            type: "union",
            variants: [
              { type: "string" },
              {
                type: "struct",
                fields: {
                  manual: { required: true, ty: { type: "boolean" } }
                }
              }
            ]
          }
        }
      }
    }
  };
}
