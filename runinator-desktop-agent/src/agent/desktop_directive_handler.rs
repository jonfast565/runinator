#[allow(unused_imports)]
use super::*;

pub(super) struct DesktopDirectiveHandler {
    pub(super) shared: SharedHandle,
    pub(super) root: PathBuf,
}

impl DirectiveHandler for DesktopDirectiveHandler {
    fn handle<'a>(
        &'a self,
        kind: &'a AgentDirectiveKind,
    ) -> Pin<Box<dyn Future<Output = DirectiveResponse> + Send + 'a>> {
        Box::pin(async move {
            match kind {
                AgentDirectiveKind::TailLogs { lines } => {
                    let count = (*lines).min(MAX_LOG_LINES);
                    let Ok(guard) = self.shared.lock() else {
                        return DirectiveResponse::failed("desktop log buffer is unavailable");
                    };
                    let logs = guard
                        .logs
                        .iter()
                        .rev()
                        .take(count)
                        .rev()
                        .cloned()
                        .collect::<Vec<_>>();
                    DirectiveResponse::completed(runinator_models::json!({ "lines": logs }))
                }
                AgentDirectiveKind::ListSandbox { path } => match resolve_sandbox(&self.root, path)
                {
                    Ok(target) => match std::fs::read_dir(target) {
                        Ok(entries) => {
                            let mut names = entries
                                .filter_map(Result::ok)
                                .take(1_000)
                                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                                .collect::<Vec<_>>();
                            names.sort();
                            DirectiveResponse::completed(
                                runinator_models::json!({ "entries": names }),
                            )
                        }
                        Err(err) => DirectiveResponse::failed(err.to_string()),
                    },
                    Err(err) => DirectiveResponse::failed(err),
                },
                AgentDirectiveKind::FetchFile { path, max_bytes } => {
                    let cap = (*max_bytes).min(8 * 1024 * 1024) as usize;
                    match resolve_sandbox(&self.root, path) {
                        Ok(target) => match std::fs::read(target) {
                            Ok(bytes) if bytes.len() <= cap => {
                                DirectiveResponse::completed(runinator_models::json!({
                                    "size": bytes.len(),
                                    "encoding": "base64",
                                    "content": base64::engine::general_purpose::STANDARD.encode(bytes),
                                }))
                            }
                            Ok(bytes) => DirectiveResponse::failed(format!(
                                "file is {} bytes, above the {} byte limit",
                                bytes.len(),
                                cap
                            )),
                            Err(err) => DirectiveResponse::failed(err.to_string()),
                        },
                        Err(err) => DirectiveResponse::failed(err),
                    }
                }
                _ => DirectiveResponse::unsupported("directive is not desktop-specific"),
            }
        })
    }
}
