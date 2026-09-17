#[allow(unused_imports)]
use super::*;

pub(super) struct OperationsScreen {
    pub(super) terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pub(super) raw: bool,
    pub(super) alternate: bool,
    pub(super) _claim: crate::TerminalClaim,
}

impl OperationsScreen {
    pub(super) fn enter() -> io::Result<Self> {
        let claim = crate::claim_terminal()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        enable_raw_mode()?;
        if let Err(error) = execute!(terminal.backend_mut(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(Self {
            terminal,
            raw: true,
            alternate: true,
            _claim: claim,
        })
    }

    pub(super) fn leave(&mut self) -> io::Result<()> {
        let mut result = Ok(());
        if self.alternate {
            result = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
            self.alternate = false;
        }
        if self.raw {
            if let Err(error) = disable_raw_mode()
                && result.is_ok()
            {
                result = Err(error);
            }
            self.raw = false;
        }
        let _ = self.terminal.show_cursor();
        result
    }
}

impl Drop for OperationsScreen {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}
