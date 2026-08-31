import type { ActionMetadata, ProviderMetadata } from "../domain/models";

export interface ProviderCatalogMatch {
  provider: ProviderMetadata;
  actions: ActionMetadata[];
}

export interface ProviderCatalogSummary {
  providers: number;
  actions: number;
  parameters: number;
  results: number;
  credentialScopes: number;
}

// Search the provider catalog as a contract, not just as a list of identifiers. Operators often
// know a credential scope, parameter, or result before they know which provider action owns it.
export function searchProviderCatalog(
  providers: ProviderMetadata[],
  query: string,
): ProviderCatalogMatch[] {
  const needle = query.trim().toLocaleLowerCase();

  if (!needle) {
    return providers.map((provider) => ({ provider, actions: provider.actions }));
  }

  return providers.flatMap((provider) => {
    const providerMatches = searchableProviderText(provider).some((value) =>
      includes(value, needle),
    );
    const actions = providerMatches
      ? provider.actions
      : provider.actions.filter((action) =>
          searchableActionText(action).some((value) => includes(value, needle)),
        );

    return providerMatches || actions.length ? [{ provider, actions }] : [];
  });
}

export function summarizeProviderCatalog(providers: ProviderMetadata[]): ProviderCatalogSummary {
  const actions = providers.flatMap((provider) => provider.actions);
  const credentialScopes = new Set(
    providers.flatMap((provider) => provider.metadata.credential_scopes),
  );

  return {
    providers: providers.length,
    actions: actions.length,
    parameters: actions.reduce((count, action) => count + action.parameters.length, 0),
    results: actions.reduce((count, action) => count + action.results.length, 0),
    credentialScopes: credentialScopes.size,
  };
}

function searchableProviderText(provider: ProviderMetadata): (string | null | undefined)[] {
  return [provider.name, provider.metadata.contract, ...provider.metadata.credential_scopes];
}

function searchableActionText(action: ActionMetadata): (string | null | undefined)[] {
  return [
    action.function_name,
    action.description,
    ...action.parameters.flatMap((parameter) => [
      parameter.name,
      parameter.label,
      parameter.description,
      JSON.stringify(parameter.ty),
    ]),
    ...action.results.flatMap((result) => [
      result.name,
      result.label,
      result.description,
      JSON.stringify(result.ty),
    ]),
  ];
}

function includes(value: string | null | undefined, needle: string): boolean {
  return value?.toLocaleLowerCase().includes(needle) ?? false;
}
