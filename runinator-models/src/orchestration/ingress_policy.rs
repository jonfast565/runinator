#[allow(unused_imports)]
use super::*;

/// Authored, provider-neutral policy carried in workflow/pipeline metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngressPolicy {
    pub scope: String,
    #[serde(default)]
    pub routes: Vec<IngressRoute>,
    /// UUID-bound config values used by direct config references in ingress predicates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setting_bindings: Vec<crate::settings::SettingBinding>,
}

impl IngressPolicy {
    /// Resolve the one policy action for an event in the admission's current lifecycle.
    /// A missing route intentionally means that the event is recorded nowhere and starts nothing.
    pub fn action_for(
        &self,
        event_type: &str,
        lifecycle: IngressLifecycle,
    ) -> Option<IngressAction> {
        self.routes
            .iter()
            .find(|route| route.event_type == event_type && route.lifecycle == lifecycle)
            .map(|route| route.action)
    }

    /// Resolve an action while honoring every predicate on the route. Callers handling a concrete
    /// event must use this form; `action_for` remains for compatibility and policy introspection.
    pub fn action_for_payload(
        &self,
        event_type: &str,
        lifecycle: IngressLifecycle,
        payload: &Value,
    ) -> Option<IngressAction> {
        self.routes_for_payload(event_type, lifecycle, payload)
            .into_iter()
            .next()
            .map(|route| route.action)
    }

    /// Return each route matching the concrete event in author order. Orchestration evaluates all
    /// dispatch matches; legacy one-action ingress behavior consumes the first match.
    pub fn routes_for_payload<'a>(
        &'a self,
        event_type: &str,
        lifecycle: IngressLifecycle,
        payload: &Value,
    ) -> Vec<&'a IngressRoute> {
        self.routes
            .iter()
            .filter(|route| {
                route.event_type == event_type
                    && route.lifecycle == lifecycle
                    && route
                        .predicates
                        .iter()
                        .all(|predicate| predicate.matches(payload))
            })
            .collect()
    }

    /// Return every matching named intent. The orchestration policy owns precedence; ingress only
    /// identifies candidates and never chooses control outcomes itself.
    pub fn dispatches_for(
        &self,
        event_type: &str,
        lifecycle: IngressLifecycle,
        payload: &Value,
    ) -> Vec<&str> {
        self.routes_for_payload(event_type, lifecycle, payload)
            .into_iter()
            .filter(|route| route.action == IngressAction::Dispatch)
            .filter_map(|route| route.intent.as_deref())
            .collect()
    }

    /// Validate the policy independently of any provider or target kind.
    pub fn validate(&self) -> Result<(), String> {
        if self.scope.trim().is_empty() {
            return Err("ingress scope must not be empty".into());
        }
        for route in &self.routes {
            if route.event_type.trim().is_empty() {
                return Err("ingress event type must not be empty".into());
            }
            if !route.action.is_allowed_when(route.lifecycle) {
                return Err(format!(
                    "ingress action '{}' is not valid when the admission is {}",
                    route.action.as_str(),
                    route.lifecycle.as_str()
                ));
            }
            for predicate in &route.predicates {
                predicate.validate()?;
            }
            match route.action {
                IngressAction::Dispatch
                    if route
                        .intent
                        .as_deref()
                        .is_none_or(|intent| intent.trim().is_empty()) =>
                {
                    return Err("a dispatch ingress route requires a non-empty intent".into());
                }
                IngressAction::Dispatch => {}
                _ if route.intent.is_some() => {
                    return Err("only a dispatch ingress route may name an intent".into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Validate the dispatch portion of an ingress policy against the orchestration policy that
    /// will consume it. Workflows and unmanaged pipelines pass `None`, because they have no named
    /// intent reducer; managed pipelines pass their snapshotted policy.
    pub fn validate_dispatches(
        &self,
        orchestration: Option<&OrchestrationPolicy>,
    ) -> Result<(), String> {
        self.validate()?;
        for route in self
            .routes
            .iter()
            .filter(|route| route.action == IngressAction::Dispatch)
        {
            let Some(orchestration) = orchestration else {
                return Err(format!(
                    "ingress dispatch intent '{}' requires an orchestration policy",
                    route.intent.as_deref().unwrap_or_default()
                ));
            };
            let intent = route.intent.as_deref().unwrap_or_default();
            if !orchestration.intents.contains_key(intent) {
                return Err(format!(
                    "ingress dispatch intent '{intent}' does not exist in the orchestration policy"
                ));
            }
        }
        Ok(())
    }

    /// Check that some installed adapter kind could emit an event carrying this policy's scope.
    ///
    /// The scope is the join between what an adapter emits and what a workflow listens for, and it
    /// is the one part of that join nothing validated: a scope no kind can produce was accepted,
    /// persisted, and hashed into the revision digest, after which the workflow simply never
    /// started. Kinds that declare no template are ignored rather than treated as unreachable, so
    /// an adapter kind that has not described its scope never blocks an apply.
    pub fn validate_reachability(&self, kinds: &[AdapterKindMetadata]) -> Result<(), String> {
        let templates: Vec<&str> = kinds
            .iter()
            .filter_map(|kind| kind.scope_template.as_deref())
            .filter(|template| !template.trim().is_empty())
            .collect();
        if templates.is_empty() {
            return Ok(());
        }
        let scope = self.scope.trim();
        if templates
            .iter()
            .any(|template| scope_matches(template, scope))
        {
            return Ok(());
        }
        Err(format!(
            "ingress scope '{scope}' cannot be emitted by any installed adapter kind; the installed kinds emit {}",
            templates.join(", ")
        ))
    }
}

/// Whether a concrete scope could have been produced by a kind's scope template. A `{placeholder}`
/// stands for one non-empty run of characters; the literal text around it has to match exactly.
fn scope_matches(template: &str, scope: &str) -> bool {
    let mut rest = scope;
    let mut remaining = template;
    let mut after_placeholder = false;
    while let Some(open) = remaining.find('{') {
        let Some(close) = remaining[open..].find('}') else {
            // an unbalanced template is not a pattern; compare it literally.
            return template == scope;
        };
        let literal = &remaining[..open];
        if after_placeholder {
            // the placeholder before this literal must consume at least one character.
            let Some(found) = rest.get(1..).and_then(|tail| tail.find(literal)) else {
                return false;
            };
            rest = &rest[found + 1 + literal.len()..];
        } else {
            let Some(stripped) = rest.strip_prefix(literal) else {
                return false;
            };
            rest = stripped;
        }
        remaining = &remaining[open + close + 1..];
        after_placeholder = true;
    }
    if after_placeholder {
        // a trailing placeholder has to consume something, then the tail literal must close it out.
        return rest.len() > remaining.len() && rest.ends_with(remaining);
    }
    rest == remaining
}
