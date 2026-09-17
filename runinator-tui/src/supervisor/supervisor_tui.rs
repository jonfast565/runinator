#[allow(unused_imports)]
use super::*;

pub struct SupervisorTui {
    pub(super) terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pub(super) mode: DashboardMode,
    pub(super) selected: usize,
    pub(super) process_count: usize,
    /// Number of data rows visible in the process table on the last draw. Keeping this alongside
    /// the selection lets left/right move through a whole visible page even after a resize.
    pub(super) process_page_rows: usize,
    pub(super) history: MetricHistory,
    pub(super) active: bool,
    pub(super) _claim: crate::TerminalClaim,
}

impl SupervisorTui {
    /// Enter the dashboard only when both streams point at a real terminal. A pipe continues to
    /// use the script-friendly table renderer instead of emitting control sequences into output.
    pub fn open(mode: DashboardMode) -> Result<Option<Self>, DynError> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Ok(None);
        }

        let claim = crate::claim_terminal()?;
        enable_raw_mode()?;
        let mut terminal = match Terminal::new(CrosstermBackend::new(io::stdout())) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = disable_raw_mode();
                return Err(error.into());
            }
        };

        if let Err(error) = execute!(terminal.backend_mut(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            let _ = terminal.show_cursor();
            return Err(error.into());
        }

        Ok(Some(Self {
            terminal,
            mode,
            selected: 0,
            process_count: 0,
            process_page_rows: 1,
            history: MetricHistory::default(),
            active: true,
            _claim: claim,
        }))
    }

    /// Run a read-only dashboard for a daemon that is already running.
    pub fn watch(mut self, state_file: &Path) -> Result<(), DynError> {
        let mut snapshot = None;
        let mut warning = None;
        let mut next_refresh = Instant::now();

        loop {
            if Instant::now() >= next_refresh {
                match read_snapshot(state_file) {
                    Ok(next) => {
                        self.observe(&next);
                        snapshot = Some(next);
                        warning = None;
                    }
                    Err(error) => {
                        warning = Some(format!("Waiting for supervisor state: {error}"));
                    }
                }
                next_refresh = Instant::now() + REFRESH_INTERVAL;
            }

            self.draw(snapshot.as_ref(), warning.as_deref())?;
            match self.poll_input(Duration::from_millis(100))? {
                DashboardAction::Continue => {}
                DashboardAction::CloseMonitor => return Ok(()),
                DashboardAction::StopSupervisor => unreachable!("monitor cannot stop a supervisor"),
            }
        }
    }

    /// Record one fresh state snapshot before it is drawn. The history is intentionally local to
    /// the UI session: snapshots are the durable contract, while rolling chart data has no reason
    /// to outlive an attached monitor.
    pub fn observe(&mut self, snapshot: &StateSnapshot) {
        self.history.observe(snapshot);
        self.process_count = snapshot.processes.len();
        if snapshot.processes.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(snapshot.processes.len() - 1);
        }
    }

    pub fn draw(
        &mut self,
        snapshot: Option<&StateSnapshot>,
        warning: Option<&str>,
    ) -> Result<(), DynError> {
        let selected = self.selected;
        let mode = self.mode;
        let history = &self.history;
        let mut process_page_rows = self.process_page_rows;
        self.terminal.draw(|frame| {
            process_page_rows = visible_process_rows(frame.area().height);
            render(
                frame,
                snapshot,
                warning,
                selected,
                history,
                mode,
                process_page_rows,
            )
        })?;
        self.process_page_rows = process_page_rows;
        Ok(())
    }

    pub fn poll_input(&mut self, timeout: Duration) -> Result<DashboardAction, DynError> {
        if !event::poll(timeout)? {
            return Ok(DashboardAction::Continue);
        }

        let Event::Key(key) = event::read()? else {
            return Ok(DashboardAction::Continue);
        };
        if key.kind == KeyEventKind::Release {
            return Ok(DashboardAction::Continue);
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.process_count > 0 {
                    self.selected = (self.selected + 1).min(self.process_count - 1);
                }
            }
            KeyCode::Left | KeyCode::PageUp | KeyCode::Char('h') => {
                self.selected = previous_process_page(self.selected, self.process_page_rows);
            }
            KeyCode::Right | KeyCode::PageDown | KeyCode::Char('l') => {
                self.selected =
                    next_process_page(self.selected, self.process_count, self.process_page_rows);
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                return Ok(match self.mode {
                    DashboardMode::Monitor => DashboardAction::CloseMonitor,
                    DashboardMode::ForegroundSupervisor => DashboardAction::StopSupervisor,
                });
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(match self.mode {
                    DashboardMode::Monitor => DashboardAction::CloseMonitor,
                    DashboardMode::ForegroundSupervisor => DashboardAction::StopSupervisor,
                });
            }
            _ => {}
        }
        Ok(DashboardAction::Continue)
    }

    pub(super) fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;

        let mut result = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        if let Err(error) = disable_raw_mode()
            && result.is_ok()
        {
            result = Err(error);
        }
        if let Err(error) = self.terminal.show_cursor()
            && result.is_ok()
        {
            result = Err(error);
        }
        result
    }
}

impl Drop for SupervisorTui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
