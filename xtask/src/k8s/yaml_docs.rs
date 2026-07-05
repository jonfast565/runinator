//! multi-document k8s yaml handling, replacing build.ps1's regex-based `Get-K8sRenderedYamlDocuments`
//! family with real yaml parsing (`kubectl kustomize` output is machine-rendered, so round-tripping
//! it through `serde_yaml` is safe — unlike hand-maintained `kustomization.yaml` files, which
//! [`super::kustomize`] edits with text surgery instead, to preserve comments/formatting).

use anyhow::Result;
use serde::Deserialize;
use serde_yaml::Value;

/// splits a `---`-separated multi-document yaml stream into individual documents.
pub fn parse_documents(rendered_yaml: &str) -> Result<Vec<Value>> {
    let mut docs = Vec::new();
    for document in serde_yaml::Deserializer::from_str(rendered_yaml) {
        let value = Value::deserialize(document)?;
        if !value.is_null() {
            docs.push(value);
        }
    }
    Ok(docs)
}

pub fn doc_kind(doc: &Value) -> Option<&str> {
    doc.get("kind")?.as_str()
}

pub fn doc_name(doc: &Value) -> Option<&str> {
    doc.get("metadata")?.get("name")?.as_str()
}

/// drops `StatefulSet` documents named in `skip_names`, leaving every other document untouched.
/// mirrors `Remove-K8sStatefulSetDocs`, used to preserve already-running postgres/rabbitmq state.
pub fn filter_out_statefulsets(docs: &[Value], skip_names: &[&str]) -> Vec<Value> {
    docs.iter()
        .filter(|doc| {
            if doc_kind(doc) != Some("StatefulSet") {
                return true;
            }
            match doc_name(doc) {
                Some(name) => !skip_names.contains(&name),
                None => true,
            }
        })
        .cloned()
        .collect()
}

/// keeps only documents whose `metadata.name` is in `names`. mirrors `Select-K8sDocsByName`, used
/// for `--command-center-only` deploys.
pub fn select_by_names(docs: &[Value], names: &[&str]) -> Vec<Value> {
    docs.iter()
        .filter(|doc| doc_name(doc).is_some_and(|name| names.contains(&name)))
        .cloned()
        .collect()
}

/// the `Deployment`/`StatefulSet` kind of the document named `name`, if any such workload exists.
pub fn workload_kind(docs: &[Value], name: &str) -> Option<String> {
    docs.iter().find_map(|doc| {
        let kind = doc_kind(doc)?;
        if kind != "Deployment" && kind != "StatefulSet" {
            return None;
        }
        (doc_name(doc) == Some(name)).then(|| kind.to_string())
    })
}

/// a `kubectl rollout status` target for `name`: its rendered kind if present, else `fallback_kind`.
pub fn rollout_target(docs: &[Value], name: &str, fallback_kind: &str) -> String {
    let kind = workload_kind(docs, name).unwrap_or_else(|| fallback_kind.to_string());
    format!("{}/{name}", kind.to_lowercase())
}

/// re-serializes a document set back into a `---`-separated yaml stream for `kubectl -f -`.
pub fn serialize_documents(docs: &[Value]) -> Result<String> {
    let mut out = String::new();
    for doc in docs {
        out.push_str("---\n");
        out.push_str(&serde_yaml::to_string(doc)?);
    }
    Ok(out)
}
