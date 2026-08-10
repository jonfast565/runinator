/// process-level facade for the command-center desktop application.
#[derive(Default)]
pub struct CommandCenterService;

impl CommandCenterService {
    pub fn new() -> Self {
        Self
    }

    pub fn run(self) {
        super::app::run();
    }
}
