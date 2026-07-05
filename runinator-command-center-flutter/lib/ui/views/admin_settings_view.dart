import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/admin_settings_service.dart';
import '../../core/services/display_preferences_service.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';

class AdminSettingsView extends ConsumerStatefulWidget {
  const AdminSettingsView({super.key});

  @override
  ConsumerState<AdminSettingsView> createState() => _AdminSettingsViewState();
}

class _AdminSettingsViewState extends ConsumerState<AdminSettingsView> {
  var _section = _AdminSection.display;
  String? _selectedLanguage;
  var _languagesOpen = true;

  @override
  Widget build(BuildContext context) {
    final display = ref.watch(displayPreferencesProvider);
    final displayNotifier = ref.read(displayPreferencesProvider.notifier);
    final admin = ref.watch(adminSettingsProvider);
    final adminNotifier = ref.read(adminSettingsProvider.notifier);
    final activeLanguage = _selectedLanguage == null
        ? null
        : admin.languages.cast<ForeignLanguageSetting?>().firstWhere((l) => l?.language == _selectedLanguage, orElse: () => null);

    return Padding(
      padding: const EdgeInsets.all(12),
      child: PanelCard(
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              width: 220,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const Padding(
                    padding: EdgeInsets.all(12),
                    child: Text('Settings', style: TextStyle(fontWeight: FontWeight.w700)),
                  ),
                  ListTile(
                    selected: _section == _AdminSection.display,
                    title: const Text('Display'),
                    onTap: () => setState(() {
                      _section = _AdminSection.display;
                      _selectedLanguage = null;
                    }),
                  ),
                  ListTile(
                    title: Row(
                      children: [
                        Icon(_languagesOpen ? Icons.expand_more : Icons.chevron_right, size: 16),
                        const SizedBox(width: 4),
                        const Expanded(child: Text('Foreign Languages')),
                        Text('${admin.languages.length}', style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
                      ],
                    ),
                    onTap: () => setState(() => _languagesOpen = !_languagesOpen),
                  ),
                  if (_languagesOpen)
                    Expanded(
                      child: ListView(
                        children: [
                          for (final runtime in admin.languages)
                            ListTile(
                              selected: _section == _AdminSection.languages && _selectedLanguage == runtime.language,
                              dense: true,
                              title: Text(runtime.label, style: const TextStyle(fontSize: 13)),
                              subtitle: Text(runtime.language, style: const TextStyle(fontSize: 11)),
                              onTap: () => setState(() {
                                _section = _AdminSection.languages;
                                _selectedLanguage = runtime.language;
                              }),
                            ),
                        ],
                      ),
                    ),
                ],
              ),
            ),
            const VerticalDivider(width: 1),
            Expanded(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: switch (_section) {
                  _AdminSection.display => _DisplayPanel(display: display, displayNotifier: displayNotifier),
                  _AdminSection.languages => _LanguagePanel(
                      runtime: activeLanguage,
                      adminNotifier: adminNotifier,
                      onRefresh: adminNotifier.refresh,
                    ),
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}

enum _AdminSection { display, languages }

class _DisplayPanel extends StatelessWidget {
  const _DisplayPanel({required this.display, required this.displayNotifier});

  final DisplayPreferencesState display;
  final DisplayPreferencesNotifier displayNotifier;

  @override
  Widget build(BuildContext context) {
    return ListView(
      children: [
        const Text('Display', style: TextStyle(fontWeight: FontWeight.w700, fontSize: 18)),
        Text('Appearance and navigation preferences stored locally.', style: TextStyle(color: AppColors.textMuted, fontSize: 12)),
        const SizedBox(height: 16),
        const Text('Theme', style: TextStyle(fontWeight: FontWeight.w600)),
        DropdownButton<AppTheme>(
          isExpanded: true,
          value: display.theme,
          items: const [
            DropdownMenuItem(value: AppTheme.system, child: Text('System')),
            DropdownMenuItem(value: AppTheme.light, child: Text('Light')),
            DropdownMenuItem(value: AppTheme.dark, child: Text('Dark')),
          ],
          onChanged: (value) {
            if (value != null) displayNotifier.setTheme(value);
          },
        ),
        const SizedBox(height: 16),
        const Text('Default landing tab', style: TextStyle(fontWeight: FontWeight.w600)),
        DropdownButton<String>(
          isExpanded: true,
          value: display.defaultTab,
          items: [
            for (final option in defaultTabOptions)
              DropdownMenuItem(value: option.value, child: Text(option.label)),
          ],
          onChanged: (value) {
            if (value != null) displayNotifier.setDefaultTab(value);
          },
        ),
      ],
    );
  }
}

class _LanguagePanel extends StatefulWidget {
  const _LanguagePanel({required this.runtime, required this.adminNotifier, required this.onRefresh});

  final ForeignLanguageSetting? runtime;
  final AdminSettingsNotifier adminNotifier;
  final Future<void> Function() onRefresh;

  @override
  State<_LanguagePanel> createState() => _LanguagePanelState();
}

class _LanguagePanelState extends State<_LanguagePanel> {
  late TextEditingController _imageController;
  late TextEditingController _setupController;

  @override
  void initState() {
    super.initState();
    _imageController = TextEditingController(text: widget.runtime?.image ?? '');
    _setupController = TextEditingController(text: widget.runtime?.setupScript ?? '');
  }

  @override
  void didUpdateWidget(covariant _LanguagePanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.runtime?.language != widget.runtime?.language) {
      _imageController.text = widget.runtime?.image ?? '';
      _setupController.text = widget.runtime?.setupScript ?? '';
    }
  }

  @override
  void dispose() {
    _imageController.dispose();
    _setupController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final runtime = widget.runtime;
    if (runtime == null) {
      return const EmptyState(message: 'Select a foreign language runtime.');
    }

    return ListView(
      children: [
        Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(runtime.label, style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 18)),
                  Text('Runtime configuration shared by workers and workflow execution.', style: TextStyle(color: AppColors.textMuted, fontSize: 12)),
                ],
              ),
            ),
            CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: widget.onRefresh),
          ],
        ),
        const SizedBox(height: 16),
        TextField(
          decoration: InputDecoration(labelText: 'Docker image', helperText: 'Default: ${runtime.defaultImage}'),
          controller: _imageController,
          onChanged: (value) => widget.adminNotifier.updateLanguageField(runtime.language, 'image', value),
        ),
        const SizedBox(height: 12),
        const Text('Setup script', style: TextStyle(fontWeight: FontWeight.w600)),
        TextField(
          controller: _setupController,
          maxLines: 8,
          decoration: const InputDecoration(border: OutlineInputBorder(), alignLabelWithHint: true),
          onChanged: (value) => widget.adminNotifier.updateLanguageField(runtime.language, 'setup_script', value),
        ),
        const SizedBox(height: 16),
        Align(
          alignment: Alignment.centerLeft,
          child: CcButton(
            icon: IconName.save,
            label: 'Save ${runtime.label}',
            variant: CcButtonVariant.primary,
            onPressed: () => widget.adminNotifier.saveLanguage(runtime.language),
          ),
        ),
      ],
    );
  }
}
