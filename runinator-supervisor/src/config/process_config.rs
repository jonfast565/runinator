#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default = "default_true")]
    pub restart_on_failure: bool,
    #[serde(default = "default_max_restarts_per_minute")]
    pub max_restarts_per_minute: u32,
    /// overrides `command` on windows; falls back to `command` when absent.
    #[serde(default)]
    pub command_windows: Option<String>,
    /// overrides `args` on windows; falls back to `args` when absent.
    #[serde(default)]
    pub args_windows: Option<Vec<String>>,
}

impl ProcessConfig {
    /// swaps in the windows-specific command/args override for the current platform, so
    /// downstream code can keep reading `command`/`args` without caring about the host os.
    pub fn resolve_for_platform(mut self) -> Self {
        #[cfg(target_os = "windows")]
        {
            if let Some(command) = self.command_windows.take() {
                self.command = command;
            }
            if let Some(args) = self.args_windows.take() {
                self.args = args;
            }
        }
        self.command_windows = None;
        self.args_windows = None;
        self
    }
}
