import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/app_service.dart';
import '../../core/services/providers_service.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';

class ProvidersView extends ConsumerStatefulWidget {
  const ProvidersView({super.key});

  @override
  ConsumerState<ProvidersView> createState() => _ProvidersViewState();
}

class _ProvidersViewState extends ConsumerState<ProvidersView> {
  String? _selectedProvider;
  String? _selectedAction;

  @override
  Widget build(BuildContext context) {
    final providersState = ref.watch(providersProvider);
    final app = ref.watch(appProvider);
    final query = ref.read(appProvider.notifier).normalizedSearch;
    final providers = providersState.providers.where((provider) {
      if (query.isEmpty) return true;
      return provider.name.toLowerCase().contains(query) ||
          provider.actions.any((action) => action.functionName.toLowerCase().contains(query));
    }).toList();

    ProviderMetadata? currentProvider;
    ActionMetadata? currentAction;
    for (final provider in providersState.providers) {
      if (provider.name == _selectedProvider) {
        currentProvider = provider;
        for (final action in provider.actions) {
          if (action.functionName == _selectedAction) {
            currentAction = action;
          }
        }
      }
    }

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        initialFirstFraction: 0.32,
        mobileShowSecond: currentProvider != null && currentAction != null,
        mobileBackTitle: currentProvider?.name,
        onMobileBack: () => setState(() {
          _selectedProvider = null;
          _selectedAction = null;
        }),
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: 'Providers',
                actions: [
                  CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => ref.read(providersProvider.notifier).fetchProvidersList()),
                ],
              ),
              Expanded(
                child: providers.isEmpty
                    ? EmptyState(message: providersState.providers.isEmpty ? 'No providers registered.' : 'No providers match "${app.searchQuery}".')
                    : ListView(
                        children: [
                          for (final provider in providers) ...[
                            ListTile(
                              selected: _selectedProvider == provider.name && _selectedAction == null,
                              title: Text(provider.name, style: const TextStyle(fontWeight: FontWeight.w700)),
                              trailing: Text('${provider.actions.length}'),
                              onTap: () => setState(() {
                                _selectedProvider = provider.name;
                                _selectedAction = null;
                              }),
                            ),
                            for (final action in provider.actions)
                              ListTile(
                                dense: true,
                                selected: _selectedProvider == provider.name && _selectedAction == action.functionName,
                                title: Padding(padding: const EdgeInsets.only(left: 16), child: Text(action.functionName)),
                                onTap: () => setState(() {
                                  _selectedProvider = provider.name;
                                  _selectedAction = action.functionName;
                                }),
                              ),
                          ],
                        ],
                      ),
              ),
            ],
          ),
        ),
        second: currentProvider == null || currentAction == null
            ? const EmptyState(message: 'Select a provider action.')
            : PanelCard(
                child: SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('${currentProvider.name}.${currentAction.functionName}', style: Theme.of(context).textTheme.titleMedium),
                      if (currentAction.description != null) ...[
                        const SizedBox(height: 8),
                        Text(currentAction.description!, style: TextStyle(color: AppColors.textMuted)),
                      ],
                      const SizedBox(height: 16),
                      const Text('Parameters', style: TextStyle(fontWeight: FontWeight.w700)),
                      const SizedBox(height: 8),
                      CcDataTable(
                        columns: const ['Name', 'Type', 'Required'],
                        rows: [
                          for (final param in currentAction.parameters)
                            [param.name, param.ty.toString(), param.required ? 'yes' : 'no'],
                        ],
                        emptyMessage: 'No parameters.',
                      ),
                    ],
                  ),
                ),
              ),
      ),
    );
  }
}
