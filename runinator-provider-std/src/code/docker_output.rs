#[allow(unused_imports)]
use super::*;

pub(super) struct DockerOutput {
    pub(super) result: runinator_sandbox::ContainerOutput,
    pub(super) output_path: PathBuf,
}
