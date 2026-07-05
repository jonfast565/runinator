import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/workflows_service.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';

class WatchExpressionsPanel extends ConsumerStatefulWidget {
  const WatchExpressionsPanel({super.key});

  @override
  ConsumerState<WatchExpressionsPanel> createState() => _WatchExpressionsPanelState();
}

class _WatchExpressionsPanelState extends ConsumerState<WatchExpressionsPanel> {
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final workflows = ref.watch(workflowsProvider);
    final notifier = ref.read(workflowsProvider.notifier);
    final workflowId = notifier.host.getWorkflowRunWorkflow()?.id;
    final expressions = workflowId == null ? const <String>[] : workflows.watchExpressionsByWorkflowId[workflowId] ?? const [];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text('Watch expressions', style: TextStyle(fontWeight: FontWeight.w700, fontSize: 12)),
        const SizedBox(height: 8),
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: _controller,
                decoration: const InputDecoration(hintText: r'e.g. $.output.ticket_id', isDense: true),
                onSubmitted: (value) {
                  notifier.runs.addWatchExpression(value);
                  _controller.clear();
                },
              ),
            ),
            const SizedBox(width: 8),
            CcButton(
              icon: IconName.plus,
              label: 'Add',
              dense: true,
              onPressed: () {
                notifier.runs.addWatchExpression(_controller.text);
                _controller.clear();
              },
            ),
          ],
        ),
        const SizedBox(height: 8),
        for (final expression in expressions)
          ListTile(
            dense: true,
            title: Text(expression, style: const TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 11)),
            trailing: IconButton(
              icon: const Icon(Icons.close, size: 14),
              onPressed: () => notifier.runs.removeWatchExpression(expression),
            ),
          ),
      ],
    );
  }
}
