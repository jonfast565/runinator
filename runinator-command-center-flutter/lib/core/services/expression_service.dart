// port of core/services/expression.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' show evaluateExpression;
import 'app_service.dart';

part 'expression_service.g.dart';

class ExpressionService {
  const ExpressionService(this._app);

  final AppNotifier _app;

  Future<Object?> evaluate(Object? expression, Object? context) =>
      _app.runOperation('Evaluating expression', () => evaluateExpression(expression, context));

  Future<Object?> evaluateSilent(Object? expression, Object? context) => evaluateExpression(expression, context);
}

@riverpod
ExpressionService expressionService(Ref ref) => ExpressionService(ref.watch(appProvider.notifier));
