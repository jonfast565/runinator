import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/admin_settings_service.dart';
import '../../core/services/display_preferences_service.dart';
import '../shared/cc_widgets.dart';

class AdminSettingsView extends ConsumerWidget {
  const AdminSettingsView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final display = ref.watch(displayPreferencesProvider);
    final displayNotifier = ref.read(displayPreferencesProvider.notifier);
    final admin = ref.watch(adminSettingsProvider);

    return Padding(
      padding: const EdgeInsets.all(12),
      child: PanelCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const PanelToolbar(title: 'Admin Settings'),
            const Text('Display', style: TextStyle(fontWeight: FontWeight.w700)),
            const SizedBox(height: 8),
            DropdownButton<AppTheme>(
              value: display.theme,
              items: const [
                DropdownMenuItem(value: AppTheme.system, child: Text('System theme')),
                DropdownMenuItem(value: AppTheme.light, child: Text('Light')),
                DropdownMenuItem(value: AppTheme.dark, child: Text('Dark')),
              ],
              onChanged: (value) {
                if (value != null) displayNotifier.setTheme(value);
              },
            ),
            const SizedBox(height: 16),
            const Text('Default landing tab', style: TextStyle(fontWeight: FontWeight.w700)),
            DropdownButton<String>(
              value: display.defaultTab,
              items: [
                for (final option in defaultTabOptions)
                  DropdownMenuItem(value: option.value, child: Text(option.label)),
              ],
              onChanged: (value) {
                if (value != null) displayNotifier.setDefaultTab(value);
              },
            ),
            const SizedBox(height: 24),
            PanelToolbar(
              title: 'Foreign Languages',
              actions: [
                CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => ref.read(adminSettingsProvider.notifier).refresh()),
              ],
            ),
            Text('Configured runtimes: ${admin.languages.length}', style: const TextStyle(fontSize: 12)),
          ],
        ),
      ),
    );
  }
}
