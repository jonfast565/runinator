import { describe, expect, it } from "vitest";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import { createPinia } from "pinia";
import TypedParameterEditor from "../TypedParameterEditor.vue";
import TypedValueEditor from "../TypedValueEditor.vue";
import type { ActionParameterMetadata } from "../../../types/models";
import { isWorkflowExpressionValue } from "../../../utils/workflow-expression-completion";

describe("TypedParameterEditor", () => {
  it("renders a direct WDL-lowered expression value as an expression editor", async () => {
    const app = createSSRApp({
      render: () => h(TypedValueEditor, {
        modelValue: { "$concat": ["ticket ", { "$ref": { params: ["ticket_id"] } }] },
        ty: { type: "string" }
      })
    });

    const html = await renderToString(app);

    expect(html).toContain("expression-editor-shell");
  });

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

  it("surfaces an existing WDL-lowered expression as an expression editor on first render", async () => {
    const modelValue = {
      summary: { "$concat": ["ticket ", { "$ref": { params: ["ticket_id"] } }] }
    };
    expect(isWorkflowExpressionValue(modelValue.summary)).toBe(true);
    const app = createSSRApp({
      render: () => h(TypedParameterEditor, {
        modelValue,
        parameters: [{
          name: "summary",
          label: "Summary",
          description: null,
          required: true,
          secret: false,
          ty: { type: "string" }
        } satisfies ActionParameterMetadata]
      })
    });
    app.use(createPinia());

    const html = await renderToString(app);

    expect(html).toContain("expression-editor-shell");
    expect(html).toContain("Expression");
    expect(html).not.toContain("placeholder");
  });

  it("uses the expression-aware value editor for generic WDL object literals", async () => {
    const app = createSSRApp({
      render: () => h(TypedParameterEditor, {
        modelValue: {
          payload: { message: { "$to_string": { "$ref": { prev: ["count"] } } } }
        },
        parameters: [{
          name: "payload",
          label: "Payload",
          description: null,
          required: false,
          secret: false,
          ty: { type: "any" }
        } satisfies ActionParameterMetadata]
      })
    });
    app.use(createPinia());

    const html = await renderToString(app);

    expect(html).toContain("expression-editor-shell");
    expect(html).toContain("Value");
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
