// the `.wdlp` pipeline surface: `pipeline "Name" { workflow "…" … "A" -> "B" on <selector> }`.
// lowers to a portable `PipelineBundle` (members and links by workflow name) for import; the web
// service resolves names to ids and materializes links as managed `chained` triggers. the reverse
// (`pipeline_to_wdlp`) re-renders a bundle so exports round-trip and the editor can format.

use std::collections::HashSet;

use runinator_models::pipelines::{
    PipelineBundle, PipelineDefaults, PipelineFailurePolicy, PipelineLinkSelector,
    PipelineLinkSpec, PipelineSpec,
};

use crate::ast::{PipelineDecl, PipelineLinkDecl};
use crate::errors::WdlError;
use crate::parser::parse_pipeline_document;

/// parse `.wdlp` source into a `PipelineBundle`. rejects empty names, empty member lists, and links
/// whose endpoints are not declared members so a compiled pipeline is always well-formed.
pub fn parse_pipeline_str(src: &str) -> Result<PipelineBundle, WdlError> {
    let decls = parse_pipeline_document(src)?;
    let mut pipelines = Vec::with_capacity(decls.len());
    for decl in &decls {
        pipelines.push(lower_pipeline(decl)?);
    }
    Ok(PipelineBundle { pipelines })
}

fn lower_pipeline(decl: &PipelineDecl) -> Result<PipelineSpec, WdlError> {
    if decl.name.trim().is_empty() {
        return Err(WdlError::syntax(
            decl.span,
            "pipeline name must not be empty",
        ));
    }
    if decl.members.is_empty() {
        return Err(WdlError::syntax(
            decl.span,
            "a pipeline must declare at least one `workflow` member",
        ));
    }
    let on_step_failure = match decl.on_failure.as_deref() {
        Some("continue") => PipelineFailurePolicy::Continue,
        // absent or "halt" -> halt (the default).
        _ => PipelineFailurePolicy::Halt,
    };
    let defaults = PipelineDefaults {
        on_step_failure,
        max_chain_depth: decl.max_depth,
        ..PipelineDefaults::default()
    };
    let members: HashSet<&str> = decl.members.iter().map(String::as_str).collect();
    let mut links = Vec::with_capacity(decl.links.len());
    for link in &decl.links {
        links.push(lower_link(link, &members, on_step_failure)?);
    }
    Ok(PipelineSpec {
        name: decl.name.clone(),
        description: decl.description.clone(),
        defaults,
        members: decl.members.clone(),
        links,
    })
}

fn lower_link(
    link: &PipelineLinkDecl,
    members: &HashSet<&str>,
    policy: PipelineFailurePolicy,
) -> Result<PipelineLinkSpec, WdlError> {
    if !members.contains(link.from.as_str()) {
        return Err(WdlError::syntax(
            link.span,
            format!(
                "link source \"{}\" is not a declared workflow member",
                link.from
            ),
        ));
    }
    if !members.contains(link.to.as_str()) {
        return Err(WdlError::syntax(
            link.span,
            format!(
                "link target \"{}\" is not a declared workflow member",
                link.to
            ),
        ));
    }
    // an explicit `on <selector>` wins; otherwise the failure policy seeds it (halt -> success,
    // continue -> complete), matching the pipeline authoring defaults.
    let on = match link.on.as_deref() {
        Some("complete") => PipelineLinkSelector::Complete,
        Some("failure") => PipelineLinkSelector::Failure,
        Some("success") => PipelineLinkSelector::Success,
        _ => match policy {
            PipelineFailurePolicy::Continue => PipelineLinkSelector::Complete,
            PipelineFailurePolicy::Halt => PipelineLinkSelector::Success,
        },
    };
    Ok(PipelineLinkSpec {
        from: link.from.clone(),
        to: link.to.clone(),
        on,
        enabled: true,
    })
}

/// render a `PipelineBundle` back into `.wdlp` source so exports round-trip and the editor can
/// format a pipeline file.
pub fn pipeline_to_wdlp(bundle: &PipelineBundle) -> String {
    let mut out = String::new();
    for (index, spec) in bundle.pipelines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!("pipeline {} {{\n", quote(&spec.name)));
        if let Some(description) = &spec.description {
            out.push_str(&format!("    description {}\n", quote(description)));
        }
        if spec.defaults.on_step_failure == PipelineFailurePolicy::Continue {
            out.push_str("    on_failure continue\n");
        }
        if let Some(max_depth) = spec.defaults.max_chain_depth {
            out.push_str(&format!("    max_depth {max_depth}\n"));
        }
        if !spec.members.is_empty() {
            out.push('\n');
            for member in &spec.members {
                out.push_str(&format!("    workflow {}\n", quote(member)));
            }
        }
        if !spec.links.is_empty() {
            out.push('\n');
            for link in &spec.links {
                out.push_str(&format!(
                    "    {} -> {} on {}\n",
                    quote(&link.from),
                    quote(&link.to),
                    link.on.as_str()
                ));
            }
        }
        out.push_str("}\n");
    }
    out
}

fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
