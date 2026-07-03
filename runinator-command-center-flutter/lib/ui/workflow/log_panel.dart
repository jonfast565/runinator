import 'package:flutter/material.dart';

import '../../core/domain/models/index.dart';
import '../theme/app_theme.dart';

class LogPanel extends StatelessWidget {
  const LogPanel({
    super.key,
    required this.chunks,
    this.lastChunkAt = 0,
    this.fallbackText = '',
    this.emptyMessage = 'No log output yet.',
  });

  final List<RunChunk> chunks;
  final int lastChunkAt;
  final String fallbackText;
  final String emptyMessage;

  @override
  Widget build(BuildContext context) {
    if (chunks.isEmpty) {
      if (fallbackText.trim().isNotEmpty) {
        return SingleChildScrollView(
          padding: const EdgeInsets.all(8),
          child: SelectableText(fallbackText, style: const TextStyle(fontFamily: 'monospace', fontSize: 11)),
        );
      }

      return Center(child: Text(emptyMessage, style: const TextStyle(color: AppColors.textMuted, fontSize: 12)));
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (lastChunkAt > 0)
          Padding(
            padding: const EdgeInsets.fromLTRB(8, 8, 8, 0),
            child: Text('Live · updated ${DateTime.fromMillisecondsSinceEpoch(lastChunkAt).toLocal()}', style: const TextStyle(fontSize: 10, color: AppColors.textMuted)),
          ),
        Expanded(
          child: ListView.builder(
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
          ),
        ),
      ],
    );
  }
}
