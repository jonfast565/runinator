//! what to run, under what limits, and what came back.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// a host directory made visible inside the container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub source: PathBuf,
    pub target: String,
    pub read_only: bool,
}

impl Mount {
    pub fn read_only(source: impl Into<PathBuf>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: true,
        }
    }

    pub fn writable(source: impl Into<PathBuf>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: false,
        }
    }
}

/// the envelope a container runs inside.
///
/// every field is an `Option` that means "leave it to the runtime" when `None`, except the two that
/// have no safe unset value: [`Self::timeout`] and [`Self::max_output_bytes`]. an unbounded run and
/// an unbounded log are both ways for one payload to take the worker with it, so neither can be
/// switched off — only widened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLimits {
    /// wall-clock deadline, enforced by the host.
    pub timeout: Duration,
    pub memory_mb: Option<i64>,
    /// cpu quota in thousandths of a core, so `1000` is one core.
    pub cpu_millis: Option<i64>,
    /// process cap, which is what stops a fork bomb.
    pub pids: Option<i64>,
    /// network access. off by default: most sandboxed code is a pure transformation, and an opt-in
    /// keeps a compromised payload from reaching the cluster it runs in.
    pub network: bool,
    /// mount the container's root filesystem read-only.
    pub read_only_root: bool,
    /// size of the writable `/tmp` mounted when the root is read-only. without it a read-only root
    /// breaks nearly every runtime, since interpreters write scratch files.
    pub tmpfs_mb: Option<i64>,
    /// the uid[:gid] to run as. `None` runs as whatever the image declares, which is usually root.
    pub user: Option<String>,
    /// drop every linux capability.
    pub drop_capabilities: bool,
    /// refuse the container any new privileges (setuid binaries cannot elevate).
    pub no_new_privileges: bool,
    /// how much of each stream is kept. output past this is dropped and the truncation reported.
    pub max_output_bytes: usize,
}

/// one mebibyte of captured output per stream, which is far more than a log ever needs and far less
/// than a payload can use to exhaust the host.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

impl Default for SandboxLimits {
    /// the hardened envelope: no network, read-only root with a small tmpfs, no capabilities, and
    /// caps on memory, cpu, and processes.
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            memory_mb: Some(512),
            cpu_millis: Some(1000),
            pids: Some(128),
            network: false,
            read_only_root: true,
            tmpfs_mb: Some(64),
            user: Some("65534:65534".to_string()),
            drop_capabilities: true,
            no_new_privileges: true,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }
}

impl SandboxLimits {
    /// the envelope `std.code` has always run under: a deadline and nothing else.
    ///
    /// this exists so porting `std.code` onto this crate is a refactor rather than a silent change
    /// to what author-written snippets may do. its `setup_script` is *for* installing dependencies,
    /// so it needs both the network and a writable root; tightening that is a deliberate decision
    /// with its own migration, not a side effect of sharing a runner.
    pub fn compatible(timeout: Duration) -> Self {
        Self {
            timeout,
            memory_mb: None,
            cpu_millis: None,
            pids: None,
            network: true,
            read_only_root: false,
            tmpfs_mb: None,
            user: None,
            drop_capabilities: false,
            no_new_privileges: false,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

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

/// what a completed container produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    /// set when output was dropped, so a caller can say so rather than presenting a partial log as
    /// the whole of one.
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub duration: Duration,
}

impl ContainerOutput {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}
