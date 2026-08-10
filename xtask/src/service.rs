/// process-level facade for workspace task command dispatch.
#[derive(Default)]
pub struct XtaskService;

impl XtaskService {
    pub fn new() -> Self {
        Self
    }

    pub fn run(self) -> anyhow::Result<()> {
        super::run_process()
    }
}
