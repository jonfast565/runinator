import 'package:flutter/material.dart';

import '../../core/domain/models/index.dart';
import '../theme/app_theme.dart';

class LogPanel extends StatelessWidget {
  const LogPanel({super.key, required this.chunks, this.emptyMessage = 'No log output yet.'});

  final List<RunChunk> chunks;
  final String emptyMessage;

  @override
  Widget build(BuildContext context) {
    if (chunks.isEmpty) {
      return Center(child: Text(emptyMessage, style: const TextStyle(color: AppColors.textMuted, fontSize: 12)));
    }

    return ListView.builder(
      padding: const EdgeInsets.all(8),
      itemCount: chunks.length,
      itemBuilder: (context, index) {
        final chunk = chunks[index];
        return Padding(
          padding: const EdgeInsets.only(bottom: 4),
          child: SelectableText(
            '[${chunk.stream}] ${chunk.content}',
            style: const TextStyle(fontFamily: 'monospace', fontSize: 11),
          ),
        );
      },
    );
  }
}
