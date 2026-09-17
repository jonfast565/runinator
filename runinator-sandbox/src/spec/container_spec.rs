#[allow(unused_imports)]
use super::*;

/// everything needed to run one container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSpec {
    pub image: String,
    /// the command, replacing the image's entrypoint arguments. empty runs the image's own command.
    pub command: Vec<String>,
    pub working_dir: Option<String>,
    /// sorted so the argv a spec produces is stable, which is what makes it assertable.
    pub env: BTreeMap<String, String>,
    pub mounts: Vec<Mount>,
    /// bytes written to the container's stdin before waiting on it.
    pub stdin: Option<Vec<u8>>,
    pub limits: SandboxLimits,
    /// prefix for the generated container name, so a stray container is traceable to its caller.
    pub name_prefix: String,
}

impl ContainerSpec {
    pub fn new(image: impl Into<String>, name_prefix: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            command: Vec::new(),
            working_dir: None,
            env: BTreeMap::new(),
            mounts: Vec::new(),
            stdin: None,
            limits: SandboxLimits::default(),
            name_prefix: name_prefix.into(),
        }
    }

    pub fn with_command(mut self, command: Vec<String>) -> Self {
        self.command = command;
        self
    }

    pub fn with_working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_mount(mut self, mount: Mount) -> Self {
        self.mounts.push(mount);
        self
    }

    pub fn with_stdin(mut self, stdin: Vec<u8>) -> Self {
        self.stdin = Some(stdin);
        self
    }

    pub fn with_limits(mut self, limits: SandboxLimits) -> Self {
        self.limits = limits;
        self
    }
}
