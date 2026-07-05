//! edits to `kustomization.yaml` files, always against a disposable copy under `target/k8s-render`
//! so the repo's checked-in overlays are never touched. Text-based line surgery (not a full yaml
//! round-trip) preserves the hand-written comments in these files; see [`super::yaml_docs`] for the
//! machine-rendered manifest side, where a real yaml round-trip is safe.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex::Regex;

use crate::fsutil::copy_dir_recursive;
use crate::paths::ensure_dir;

pub struct ImageRef {
    pub name: String,
    pub tag: String,
}

/// splits `name:tag` on the last `:` that follows the last `/`, so a registry host's own port
/// (`host:5000/name`) is not mistaken for the tag separator.
pub fn split_image_reference(reference: &str) -> Result<ImageRef> {
    let last_slash = reference.rfind('/').map(|i| i as isize).unwrap_or(-1);
    let last_colon = reference.rfind(':').map(|i| i as isize).unwrap_or(-1);
    if last_colon <= last_slash {
        bail!("image reference '{reference}' must include a tag.");
    }
    let last_colon = last_colon as usize;
    Ok(ImageRef {
        name: reference[..last_colon].to_string(),
        tag: reference[last_colon + 1..].to_string(),
    })
}

/// copies `deploy/k8s` into `target/k8s-render/k8s` and returns the path to `overlay_path`'s
/// counterpart inside that copy, so image/component edits never dirty the checked-in overlay.
pub fn render_overlay_copy(workspace_root: &Path, overlay_path: &Path) -> Result<PathBuf> {
    let k8s_root = workspace_root.join("deploy/k8s");
    let k8s_root = k8s_root
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", k8s_root.display()))?;

    let resolved_overlay = if overlay_path.is_absolute() {
        overlay_path.to_path_buf()
    } else {
        workspace_root.join(overlay_path)
    };
    let resolved_overlay = resolved_overlay
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", resolved_overlay.display()))?;

    if !resolved_overlay.starts_with(&k8s_root) {
        bail!(
            "image overrides require an overlay under {}",
            k8s_root.display()
        );
    }

    let render_root = workspace_root.join("target/k8s-render");
    if render_root.exists() {
        std::fs::remove_dir_all(&render_root)
            .with_context(|| format!("failed to clear {}", render_root.display()))?;
    }
    ensure_dir(&render_root)?;
    let rendered_k8s_root = render_root.join("k8s");
    copy_dir_recursive(&k8s_root, &rendered_k8s_root)?;

    let relative_overlay = resolved_overlay.strip_prefix(&k8s_root)?;
    Ok(rendered_k8s_root.join(relative_overlay))
}

/// rewrites each `images:` entry in `overlay_path`'s kustomization to point at the built tag in
/// `image_map` (name -> full `name:tag` reference), replacing any existing `newName`/`newTag`
/// override lines that followed it. errors if `image_map` names an image the kustomization lacks.
pub fn set_overlay_images(overlay_path: &Path, image_map: &HashMap<String, String>) -> Result<()> {
    let kustomization_path = overlay_path.join("kustomization.yaml");
    let content = std::fs::read_to_string(&kustomization_path).with_context(|| {
        format!(
            "kustomization file not found at {}",
            kustomization_path.display()
        )
    })?;
    let lines: Vec<&str> = content.lines().collect();

    let name_line_re = Regex::new(r"^(?P<indent>\s*)-\s+name:\s+(?P<name>\S+)\s*$")?;
    let override_line_re = Regex::new(r"^\s+new(Name|Tag):\s+")?;

    let mut updated: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some(caps) = name_line_re.captures(line) {
            let name = &caps["name"];
            if let Some(reference) = image_map.get(name) {
                let indent = &caps["indent"];
                let image = split_image_reference(reference)?;
                updated.push(line.to_string());
                updated.push(format!("{indent}  newName: {}", image.name));
                updated.push(format!("{indent}  newTag: {}", image.tag));
                seen.insert(name.to_string());
                index += 1;
                while index < lines.len() && override_line_re.is_match(lines[index]) {
                    index += 1;
                }
                continue;
            }
        }
        updated.push(line.to_string());
        index += 1;
    }

    for name in image_map.keys() {
        if !seen.contains(name) {
            bail!(
                "kustomization at {} does not define image '{name}'",
                kustomization_path.display()
            );
        }
    }

    std::fs::write(&kustomization_path, format!("{}\n", updated.join("\n")))
        .with_context(|| format!("failed to write {}", kustomization_path.display()))
}

/// adds `component_rel_path` (relative to the render copy's `deploy/k8s`, e.g.
/// `components/direct-ingress`) to `overlay_path`'s `components:` list, creating that list if
/// the kustomization does not already have one.
pub fn add_component(
    workspace_root: &Path,
    overlay_path: &Path,
    component_rel_path: &str,
) -> Result<()> {
    let kustomization_path = overlay_path.join("kustomization.yaml");
    if !kustomization_path.exists() {
        bail!(
            "kustomization file not found at {}",
            kustomization_path.display()
        );
    }

    let copied_k8s_root = workspace_root.join("target/k8s-render/k8s");
    let component_abs = copied_k8s_root.join(component_rel_path);
    if !component_abs.exists() {
        bail!(
            "kustomize component not found at {}",
            component_abs.display()
        );
    }

    let relative = relative_path(overlay_path, &component_abs);
    let relative_str = relative.to_string_lossy().replace('\\', "/");

    let content = std::fs::read_to_string(&kustomization_path)?;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let components_re = Regex::new(r"^components:\s*$")?;

    match lines.iter().position(|line| components_re.is_match(line)) {
        Some(index) => lines.insert(index + 1, format!("  - {relative_str}")),
        None => {
            lines.push("components:".to_string());
            lines.push(format!("  - {relative_str}"));
        }
    }

    std::fs::write(&kustomization_path, format!("{}\n", lines.join("\n")))
        .with_context(|| format!("failed to write {}", kustomization_path.display()))
}

/// relative path from directory `from_dir` to `to_path`, assuming both are absolute and share a
/// common ancestor (true here: both live under the same `target/k8s-render/k8s` copy).
fn relative_path(from_dir: &Path, to_path: &Path) -> PathBuf {
    let from_components: Vec<_> = from_dir.components().collect();
    let to_components: Vec<_> = to_path.components().collect();
    let common_len = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();
    for _ in common_len..from_components.len() {
        result.push("..");
    }
    for component in &to_components[common_len..] {
        result.push(component.as_os_str());
    }
    result
}
