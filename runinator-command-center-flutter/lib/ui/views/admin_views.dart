import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/dead_letters_service.dart';
import '../../core/services/audit_log_service.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../shared/split_pane.dart';

class DeadLettersView extends ConsumerStatefulWidget {
  const DeadLettersView({super.key});

  @override
  ConsumerState<DeadLettersView> createState() => _DeadLettersViewState();
}

class _DeadLettersViewState extends ConsumerState<DeadLettersView> {
  String? _channel;
  List<Map<String, Object?>> _rows = const [];
  var _loading = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final rows = await ref.read(deadLettersServiceProvider).list(channel: _channel);
      setState(() => _rows = rows.cast());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(12),
      child: PanelCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PanelToolbar(
              title: 'Dead Letters',
              actions: [
                DropdownButton<String?>(
                  value: _channel,
                  items: const [
                    DropdownMenuItem(value: null, child: Text('All channels')),
                    DropdownMenuItem(value: 'result', child: Text('result')),
                    DropdownMenuItem(value: 'ingress', child: Text('ingress')),
                  ],
                  onChanged: (value) {
                    setState(() => _channel = value);
                    _load();
                  },
                ),
                CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: _loading ? null : _load),
              ],
            ),
            if (_loading) const LinearProgressIndicator(minHeight: 2),
            Expanded(
              child: _rows.isEmpty
                  ? const EmptyState(message: 'No dead letters.')
                  : ListView.builder(
                      itemCount: _rows.length,
                      itemBuilder: (context, index) {
                        final row = _rows[index];
                        return ExpansionTile(
                          title: Text(displayValue(row['channel'] ?? row['id'])),
                          subtitle: Text(displayValue(row['created_at'])),
                          children: [
                            Padding(
                              padding: const EdgeInsets.all(12),
                              child: JsonEditor(value: row.toString(), onChanged: (_) {}, readOnly: true),
                            ),
                          ],
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

class AuditLogView extends ConsumerStatefulWidget {
  const AuditLogView({super.key});

  @override
  ConsumerState<AuditLogView> createState() => _AuditLogViewState();
}

class _AuditLogViewState extends ConsumerState<AuditLogView> {
  final _actionFilter = TextEditingController();
  List<Map<String, Object?>> _rows = const [];
  var _loading = false;

  @override
  void dispose() {
    _actionFilter.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final rows = await ref.read(auditLogServiceProvider).list(
            action: _actionFilter.text.trim().isEmpty ? null : _actionFilter.text.trim(),
          );
      setState(() => _rows = rows.cast());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(12),
      child: PanelCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PanelToolbar(
              title: 'Audit Log',
              actions: [
                SizedBox(
                  width: 180,
                  child: TextField(
                    controller: _actionFilter,
                    decoration: const InputDecoration(hintText: 'Filter action'),
                    onSubmitted: (_) => _load(),
                  ),
                ),
                CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: _loading ? null : _load),
              ],
            ),
            if (_loading) const LinearProgressIndicator(minHeight: 2),
            Expanded(
              child: CcDataTable(
                columns: const ['Time', 'Action', 'Outcome', 'Actor', 'Resource'],
                rows: [
                  for (final row in _rows)
                    [
                      displayValue(row['created_at']),
                      displayValue(row['action']),
                      displayValue(row['outcome']),
                      displayValue(row['actor']),
                      displayValue(row['resource']),
                    ],
                ],
                emptyMessage: 'No audit entries.',
              ),
            ),
          ],
        ),
      ),
    );
  }
}
