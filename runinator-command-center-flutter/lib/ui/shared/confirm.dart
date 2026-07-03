import 'package:flutter/material.dart';

import '../../core/services/gates_service.dart';

class FlutterConfirmContext implements ConfirmContext {
  FlutterConfirmContext(this.context);

  final BuildContext context;

  @override
  bool confirm(String message) => true;

  @override
  String? prompt(String message) => null;

  Future<bool> confirmAsync(String message) async {
    final result = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Confirm'),
        content: Text(message),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('OK')),
        ],
      ),
    );
    return result ?? false;
  }

  Future<String?> promptAsync(String message) async {
    final controller = TextEditingController();
    final result = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Input required'),
        content: TextField(controller: controller, decoration: InputDecoration(hintText: message)),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(context, controller.text), child: const Text('OK')),
        ],
      ),
    );
    final trimmed = result?.trim();
    return trimmed == null || trimmed.isEmpty ? null : trimmed;
  }
}
