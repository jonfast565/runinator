import { defaultApi, type ExpressionApi } from "../api/ports/expression";

import type { AppService } from "./app";

export function createExpressionService(app: AppService, api: ExpressionApi = defaultApi) {
  return {
    evaluate(expression: unknown, context: unknown) {
      return app.runOperation("Evaluating expression", () =>
        api.evaluateExpression(expression, context),
      );
    },
    evaluateSilent(expression: unknown, context: unknown) {
      return api.evaluateExpression(expression, context);
    },
  };
}

export type ExpressionService = ReturnType<typeof createExpressionService>;
