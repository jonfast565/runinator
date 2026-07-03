// port of core/utils/inputs.ts.

import '../domain/models/index.dart';
import 'values.dart';

bool isInputWaitingStatus(Object? status) =>
    ['waiting', 'input_required', 'pending'].contains(_normalizeStatus(status));

Object? inputValueFromNodeRun(WorkflowNodeRun nodeRun) =>
    nodeRun.outputJson ?? nodeRun.state?['input'];

String _normalizeStatus(Object? status) => displayValue(status).trim().toLowerCase().replaceAll('-', '_');
