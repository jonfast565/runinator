import { evaluateExpression } from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createExpressionService(app: AppService) {
  return {
    evaluate(expression: unknown, context: unknown) {
      return app.runOperation("Evaluating expression", () => evaluateExpression(expression, context));
    },
    evaluateSilent(expression: unknown, context: unknown) {
      return evaluateExpression(expression, context);
    },
  };
}

export type ExpressionService = ReturnType<typeof createExpressionService>;
