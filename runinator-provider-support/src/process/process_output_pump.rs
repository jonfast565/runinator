#[allow(unused_imports)]
use super::*;

pub struct ProcessOutputPump {
    pub(super) stdout: Option<JoinHandle<String>>,
    pub(super) stderr: Option<JoinHandle<String>>,
}

impl ProcessOutputPump {
    /// Take both piped streams from `child` and begin draining them immediately.
    pub fn start(
        child: &mut Child,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> std::io::Result<Self> {
        Self::start_with_retention(child, sink, true)
    }

    /// Begin draining and streaming without retaining a second in-memory copy.
    pub fn start_discarding(
        child: &mut Child,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> std::io::Result<Self> {
        Self::start_with_retention(child, sink, false)
    }

    pub(super) fn start_with_retention(
        child: &mut Child,
        sink: Option<Arc<dyn ProviderEventSink>>,
        retain: bool,
    ) -> std::io::Result<Self> {
        let stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::other("child stdout is unavailable; configure it as piped")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            std::io::Error::other("child stderr is unavailable; configure it as piped")
        })?;
        Ok(Self {
            stdout: Some(spawn_stream(stdout, "stdout", sink.clone(), retain)),
            stderr: Some(spawn_stream(stderr, "stderr", sink, retain)),
        })
    }

    /// Wait for both streams to reach EOF and return the retained text.
    pub fn finish(mut self) -> ProcessOutput {
        ProcessOutput {
            stdout: join_stream(self.stdout.take()),
            stderr: join_stream(self.stderr.take()),
        }
    }
}
