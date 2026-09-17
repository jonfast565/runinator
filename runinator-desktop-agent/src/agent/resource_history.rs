#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default)]
pub struct ResourceHistory {
    pub(super) samples: VecDeque<ResourceSample>,
}

impl ResourceHistory {
    pub fn push(&mut self, sample: ResourceSample) {
        if self.samples.len() == RESOURCE_HISTORY_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn samples(&self) -> impl Iterator<Item = &ResourceSample> {
        self.samples.iter()
    }
}
