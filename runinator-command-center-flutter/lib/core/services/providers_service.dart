// port of core/services/providers.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' show fetchProviders;
import '../domain/models/index.dart';
import '../utils/format.dart' show errorMessage;

part 'providers_service.g.dart';

class ProvidersState {
  const ProvidersState({
    required this.providers,
    required this.loading,
    this.error,
    required this.focusedProvider,
    required this.focusedAction,
  });

  final List<ProviderMetadata> providers;
  final bool loading;
  final String? error;
  final String focusedProvider;
  final String focusedAction;

  ProvidersState copyWith({
    List<ProviderMetadata>? providers,
    bool? loading,
    Object? error = _unset,
    String? focusedProvider,
    String? focusedAction,
  }) =>
      ProvidersState(
        providers: providers ?? this.providers,
        loading: loading ?? this.loading,
        error: identical(error, _unset) ? this.error : error as String?,
        focusedProvider: focusedProvider ?? this.focusedProvider,
        focusedAction: focusedAction ?? this.focusedAction,
      );
}

const Object _unset = Object();

ProvidersState _initialProvidersState() => const ProvidersState(
      providers: [],
      loading: false,
      error: null,
      focusedProvider: '',
      focusedAction: '',
    );

@riverpod
class ProvidersNotifier extends _$ProvidersNotifier {
  @override
  ProvidersState build() => _initialProvidersState();

  void focusProviderAction(String provider, [String action = '']) {
    state = state.copyWith(focusedProvider: provider, focusedAction: action);
  }

  Future<void> fetchProvidersList() async {
    state = state.copyWith(loading: true, error: null);

    try {
      final response = await fetchProviders();
      final providers = response.map(_normalizeProvider).where((p) => p.name.isNotEmpty).toList()
        ..sort((left, right) => left.name.compareTo(right.name));
      state = state.copyWith(providers: providers, loading: false);
    } catch (err) {
      final message = errorMessage(err);
      state = state.copyWith(error: message.isNotEmpty ? message : 'Failed to fetch providers', loading: false);
    }
  }

  void clearProviders() {
    state = _initialProvidersState();
  }
}

ProviderMetadata _normalizeProvider(ProviderMetadata provider) => ProviderMetadata(
      name: provider.name,
      actions: [...provider.actions]..sort((left, right) => left.functionName.compareTo(right.functionName)),
      metadata: provider.metadata,
    );
