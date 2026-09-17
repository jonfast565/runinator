#[allow(unused_imports)]
use super::*;

pub(super) struct CodexHome {
    pub(super) path: PathBuf,
    pub(super) temporary: Option<tempfile::TempDir>,
    pub(super) persistent: bool,
}

impl CodexHome {
    pub(super) fn prepare(
        request: &ProviderExecutionRequest,
        params: &CodexParams,
    ) -> Result<Self, SendableError> {
        let (path, temporary, persistent) = match (
            request.workspace_path.as_deref(),
            params.session_slot.as_deref(),
        ) {
            (Some(workspace), Some(slot)) => {
                let git = Path::new(workspace).join(".git");
                if !git.is_dir() {
                    return Err(
                        CODEX_INPUT.error("persistent Codex sessions require a Git workspace")
                    );
                }
                (git.join("runinator").join("codex").join(slot), None, true)
            }
            _ => {
                let temp = tempfile::tempdir().map_err(|error| CODEX_INPUT.error(error))?;
                (temp.path().to_owned(), Some(temp), false)
            }
        };
        fs::create_dir_all(&path).map_err(|error| CODEX_INPUT.error(error))?;
        if let Some(profile) = &request.execution_profile
            && let Some(profile_home) = &profile.home
        {
            let source = Path::new(profile_home).join(".codex/auth.json");
            if source.is_file() {
                fs::copy(source, path.join("auth.json"))
                    .map_err(|error| CODEX_INPUT.error(error))?;
            }
        }
        if params.mission_mcp {
            let mission_id = params.mission_id.as_deref().unwrap_or_default();
            let config = format!(
                "[mcp_servers.runinator_mission]\ncommand = \"runinatorctl\"\nargs = [\"mcp\", \"--mission-only\", \"--mission-id\", \"{mission_id}\"]\nrequired = true\n"
            );
            fs::write(path.join("config.toml"), config)
                .map_err(|error| CODEX_INPUT.error(error))?;
        }
        Ok(Self {
            path,
            temporary,
            persistent,
        })
    }

    pub(super) fn manifest(&self) -> PathBuf {
        self.path.join("runinator-thread.json")
    }

    pub(super) fn load_thread(&self) -> Result<Option<String>, SendableError> {
        if !self.persistent || !self.manifest().is_file() {
            return Ok(None);
        }
        let value: Value = serde_json::from_slice(&fs::read(self.manifest())?)
            .map_err(|error| CODEX_PROTOCOL.error(error))?;
        Ok(value
            .get("thread_id")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    pub(super) fn save_thread(&self, thread_id: &str) -> Result<(), SendableError> {
        if self.persistent {
            fs::write(
                self.manifest(),
                serde_json::to_vec(&json!({ "thread_id": thread_id }))?,
            )?;
        }
        Ok(())
    }

    pub(super) fn scrub(&mut self) {
        for name in ["auth.json", "config.toml"] {
            let _ = fs::remove_file(self.path.join(name));
        }
    }
}

impl Drop for CodexHome {
    fn drop(&mut self) {
        self.scrub();
        let _ = self.temporary.as_ref();
    }
}
