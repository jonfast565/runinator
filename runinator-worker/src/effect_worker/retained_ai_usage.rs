#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(crate) struct RetainedAiUsage(pub(super) StdMutex<Option<runinator_models::ai_usage::AiUsage>>);

impl RetainedAiUsage {
    pub(crate) fn retain(&self, usage: runinator_models::ai_usage::AiUsage) {
        if let Ok(mut retained) = self.0.lock() {
            *retained = Some(usage);
        }
    }

    pub(crate) fn take(&self) -> Option<runinator_models::ai_usage::AiUsage> {
        self.0.lock().ok()?.take()
    }
}
