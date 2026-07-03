import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/services/app_service.dart';
import '../theme/app_theme.dart';

class ToastHostOverlay extends ConsumerWidget {
  const ToastHostOverlay({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final toasts = ref.watch(appProvider).toasts;

    return Align(
      alignment: Alignment.bottomRight,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.end,
          children: [
            for (final toast in toasts)
              Container(
                margin: const EdgeInsets.only(top: 8),
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                constraints: const BoxConstraints(maxWidth: 360),
                decoration: BoxDecoration(
                  color: AppColors.surface,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: AppColors.border),
                  boxShadow: const [BoxShadow(color: Color(0x2E17202A), blurRadius: 18, offset: Offset(0, 8))],
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (toast.kind == ToastKind.loading)
                      const SizedBox(width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2))
                    else
                      Icon(_iconFor(toast.kind), size: 16, color: _colorFor(toast.kind)),
                    const SizedBox(width: 8),
                    Expanded(child: Text(toast.text, style: const TextStyle(fontSize: 12))),
                    IconButton(
                      visualDensity: VisualDensity.compact,
                      icon: const Icon(Icons.close, size: 14),
                      onPressed: () => ref.read(appProvider.notifier).dismissToast(toast.id),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }

  IconData _iconFor(ToastKind kind) {
    switch (kind) {
      case ToastKind.success:
        return Icons.check_circle_outline;
      case ToastKind.error:
        return Icons.error_outline;
      case ToastKind.info:
      case ToastKind.loading:
        return Icons.info_outline;
    }
  }

  Color _colorFor(ToastKind kind) {
    switch (kind) {
      case ToastKind.success:
        return AppColors.successFg;
      case ToastKind.error:
        return AppColors.dangerFg;
      case ToastKind.info:
      case ToastKind.loading:
        return AppColors.infoFg;
    }
  }
}
