#[allow(unused_imports)]
use super::*;

pub struct ActionCatalog<'a> {
    pub(super) actions: HashMap<(&'a str, &'a str), &'a ActionMetadata>,
}

impl<'a> ActionCatalog<'a> {
    /// index every action the given providers expose.
    pub fn new(providers: &'a [ProviderMetadata]) -> Self {
        let actions = providers
            .iter()
            .flat_map(|provider| {
                provider.actions.iter().map(move |action| {
                    (
                        (provider.name.as_str(), action.function_name.as_str()),
                        action,
                    )
                })
            })
            .collect();
        Self { actions }
    }

    /// the action a `provider.function` pair names, if it is registered.
    pub fn get(&self, provider: &str, function: &str) -> Option<&'a ActionMetadata> {
        self.actions.get(&(provider, function)).copied()
    }
}
