import 'package:flutter/material.dart';

import '../../core/utils/workflow_references.dart';
import '../theme/app_theme.dart';

class ReferencePicker extends StatefulWidget {
  const ReferencePicker({
    super.key,
    required this.groups,
    required this.onInsert,
    required this.onTransform,
  });

  final List<ReferenceGroup> groups;
  final ValueChanged<String> onInsert;
  final void Function(String kind) onTransform;

  @override
  State<ReferencePicker> createState() => _ReferencePickerState();
}

class _ReferencePickerState extends State<ReferencePicker> {
  var _query = '';

  List<ReferenceGroup> get _filtered {
    final needle = _query.trim().toLowerCase();
    if (needle.isEmpty) return widget.groups;

    return [
      for (final group in widget.groups)
        ReferenceGroup(
          title: group.title,
          references: group.references.where((ref) => ref.label.toLowerCase().contains(needle)).toList(),
        ),
    ].where((group) => group.references.isNotEmpty).toList();
  }

  @override
  Widget build(BuildContext context) {
    final groups = _filtered;

    return Container(
      decoration: BoxDecoration(
        border: Border.all(color: AppColors.border),
        borderRadius: BorderRadius.circular(6),
        color: AppColors.surfaceSubtle,
      ),
      padding: const EdgeInsets.all(8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          TextField(
            decoration: const InputDecoration(hintText: 'Filter references…', isDense: true),
            onChanged: (value) => setState(() => _query = value),
          ),
          const SizedBox(height: 8),
          if (groups.isEmpty)
            Text('No references in scope.', style: TextStyle(fontSize: 12, color: AppColors.textMuted))
          else
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 180),
              child: ListView(
                shrinkWrap: true,
                children: [
                  for (final group in groups) ...[
                    Text(group.title, style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 11)),
                    for (final ref in group.references)
                      TextButton(
                        style: TextButton.styleFrom(padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2)),
                        onPressed: () => widget.onInsert(ref.insert),
                        child: Row(
                          children: [
                            Expanded(child: Text(ref.label, style: const TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 12))),
                            Text(ref.type, style: TextStyle(fontSize: 10, color: AppColors.textMuted)),
                          ],
                        ),
                      ),
                  ],
                ],
              ),
            ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            children: [
              Text('Wrap selection:', style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
              for (final entry in [
                ('string', 'string()'),
                ('json', 'json()'),
                ('coalesce', '??'),
                ('concat', '++'),
              ])
                OutlinedButton(
                  style: OutlinedButton.styleFrom(minimumSize: const Size(0, 28), padding: const EdgeInsets.symmetric(horizontal: 8)),
                  onPressed: () => widget.onTransform(entry.$1),
                  child: Text(entry.$2, style: const TextStyle(fontSize: 11)),
                ),
            ],
          ),
        ],
      ),
    );
  }
}
