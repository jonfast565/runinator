//! the desktop agent's control surface: a small window to configure the sandbox folder, start the
//! worker loop — or cancel a start still coming up — stop it, and watch its status. Its live
//! dashboard mirrors the terminal host's operational view without turning this into a workflow
//! editor. The window's close button asks whether to hide it behind the tray icon or quit.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::agent::{
    self, AgentConfig, AgentMetrics, AgentStatus, ConnectionState, Control, SharedHandle,
};
use crate::config::{self, LogLevel, WindowCloseAction};
use crate::logging;
use crate::tray::{AgentTray, TrayAction, TrayColor};
use runinator_worker::ActionOutcome;

// presets offered by the optional-label type-ahead. `pool=desktop` and `runner=desktop` are shown
// separately as fixed identity labels, so suggestions only cover user-configurable routing facts.
const LABEL_SUGGESTIONS: &[&str] = &["zone=home", "capability=desktop"];
const REQUIRED_LABELS: &[&str] = &["pool=desktop", "runner=desktop"];

/// a per-frame copy of the shared agent state the GUI renders from, taken under one short lock.
struct Snapshot {
    status: AgentStatus,
    connection: ConnectionState,
    metrics: AgentMetrics,
    busy: bool,
    /// which of Start / Cancel startup / Stop this phase warrants; see [`agent::Control`].
    control: Control,
    agent_activity: String,
    agent_activity_age: Duration,
    worker_activity: String,
    worker_activity_age: Duration,
    resources: Vec<agent::ResourceSample>,
}

struct RuntimeDashboard<'a> {
    status: &'a AgentStatus,
    metrics: &'a AgentMetrics,
    agent_activity: &'a str,
    agent_activity_age: Duration,
    worker_activity: &'a str,
    worker_activity_age: Duration,
    resources: &'a [agent::ResourceSample],
    uptime: Duration,
    capacity: usize,
    service_url: &'a str,
}

// the status-dot palette. amber and red are the two the operator is meant to tell apart at a
// glance: amber is "still trying", red is "stopped, and not coming back without you".
const DOT_GRAY: egui::Color32 = egui::Color32::from_rgb(130, 130, 130);
const DOT_BLUE: egui::Color32 = egui::Color32::from_rgb(45, 140, 200);
const DOT_GREEN: egui::Color32 = egui::Color32::from_rgb(64, 180, 96);
const DOT_AMBER: egui::Color32 = egui::Color32::from_rgb(220, 170, 45);
const DOT_RED: egui::Color32 = egui::Color32::from_rgb(210, 70, 70);

/// how a connection state renders: header dot text + color, and the matching tray color/tooltip.
struct StatusPresentation {
    label: String,
    color: egui::Color32,
    tray_color: TrayColor,
    tooltip: String,
}

fn present_status(connection: &ConnectionState, busy: bool) -> StatusPresentation {
    // a start/stop transition in flight reads as "working" regardless of the underlying phase.
    if busy {
        return StatusPresentation {
            label: "working…".to_string(),
            color: DOT_AMBER,
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — working…".to_string(),
        };
    }
    match connection {
        ConnectionState::Stopped => StatusPresentation {
            label: "stopped".to_string(),
            color: DOT_GRAY,
            tray_color: TrayColor::Idle,
            tooltip: "Runinator Desktop Agent — stopped".to_string(),
        },
        ConnectionState::Registering => StatusPresentation {
            label: "registering…".to_string(),
            color: DOT_BLUE,
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — registering…".to_string(),
        },
        ConnectionState::Connecting => StatusPresentation {
            label: "connecting…".to_string(),
            color: DOT_BLUE,
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — connecting…".to_string(),
        },
        ConnectionState::Connected => StatusPresentation {
            label: "running".to_string(),
            color: DOT_GREEN,
            tray_color: TrayColor::Connected,
            tooltip: "Runinator Desktop Agent — running".to_string(),
        },
        ConnectionState::Reconnecting {
            retry_secs,
            attempt,
            max_attempts,
        } => StatusPresentation {
            label: format!("reconnecting{}", attempt_suffix(*attempt, *max_attempts)),
            color: DOT_AMBER,
            tray_color: TrayColor::Reconnecting,
            tooltip: format!(
                "Runinator Desktop Agent — reconnecting{} (retry in {retry_secs}s)",
                attempt_suffix(*attempt, *max_attempts)
            ),
        },
        ConnectionState::Disconnected { attempts, reason } => StatusPresentation {
            label: "disconnected".to_string(),
            color: DOT_RED,
            tray_color: TrayColor::Disconnected,
            tooltip: format!(
                "Runinator Desktop Agent — disconnected after {attempts} attempts ({reason})"
            ),
        },
        ConnectionState::ReenrollmentRequired { reason } => StatusPresentation {
            label: "re-enrollment required".to_string(),
            color: DOT_RED,
            tray_color: TrayColor::Disconnected,
            tooltip: format!("Runinator Desktop Agent — credential rejected ({reason})"),
        },
    }
}

// " 3/10", or nothing when the agent retries indefinitely — the count only means something against
// a budget.
fn attempt_suffix(attempt: u32, max_attempts: Option<u32>) -> String {
    match max_attempts {
        Some(max) => format!(" {attempt}/{max}"),
        None => String::new(),
    }
}

/// the first reason the current draft can't start, or `None` when it's good to go. drives the Start
/// button's enabled state and its disabled-hover explanation, so a misconfiguration is caught here
/// rather than after the worker loop has already spun up.
fn validate_config(draft: &AgentConfig) -> Option<String> {
    let service_url = draft.service_url.trim();
    if service_url.is_empty() {
        return Some("Set a service URL.".to_string());
    }
    match reqwest::Url::parse(service_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => {}
        Ok(_) => return Some("Service URL must be http:// or https://.".to_string()),
        Err(_) => return Some("Service URL is not a valid URL.".to_string()),
    }

    let root = draft.sandbox_root.trim();
    if root.is_empty() {
        return Some("Choose a sandbox folder.".to_string());
    }
    if !std::path::Path::new(root).is_dir() {
        return Some("Sandbox folder does not exist.".to_string());
    }

    let working_dir = draft.console_working_dir.trim();
    if !working_dir.is_empty() && !std::path::Path::new(working_dir).is_dir() {
        return Some("Working directory does not exist.".to_string());
    }

    if draft.broker_mode == config::BrokerMode::Direct {
        if draft.direct_broker_backend.trim().is_empty() {
            return Some("Set a broker backend for Direct mode.".to_string());
        }
        if draft.direct_broker_endpoint.trim().is_empty() {
            return Some("Set a broker endpoint for Direct mode.".to_string());
        }
    }

    None
}

pub struct DesktopAgentApp {
    rt: tokio::runtime::Runtime,
    shared: SharedHandle,
    // the editable draft bound to the form; only applied to the running agent on "Start".
    draft: AgentConfig,
    // in-progress text for the next label tag; separate from `draft` since it is editor-only state,
    // never persisted or sent to the agent until committed as a tag.
    label_input: String,
    // A one-time replacement credential is deliberately separate from `draft`: enrollment tokens
    // must only ever be held in memory and are never written to the saved desktop-agent settings.
    reenrollment_token: String,
    reenrollment_dialog: bool,
    // `None` when the platform tray failed to initialize; the window is then the only way in, so it
    // remains visible rather than stranding the user with no way to reach it.
    tray: Option<AgentTray>,
    // last tray icon/tooltip pushed, so we only touch the platform tray when the state actually
    // changes rather than on every 400ms repaint.
    last_tray_signature: Option<String>,
    // case-insensitive substring filter applied to the log console; empty shows everything.
    log_filter: String,
    // Keep the console at its live edge while entries arrive. An operator can pause following to
    // inspect older output without new lines pulling the viewport away.
    follow_logs: bool,
    // set once "Exit" is chosen, so the window's own close-intercept doesn't cancel our own Close cmd.
    quitting: bool,
    // shown after a title-bar close request, so closing the control window never silently changes
    // whether this machine continues accepting desktop work.
    exit_dialog: bool,
    exit_dont_ask_again: bool,
    /// The GUI's equivalent of the terminal dashboard uptime, measured from this control surface
    /// coming up rather than from the most recent worker start.
    started_at: Instant,
}

impl DesktopAgentApp {
    /// builds the app, including the tray icon. must run on the main thread after the platform event
    /// loop has started — `cc` (handed to eframe's app-creator closure) guarantees that timing.
    /// `shared` is the same state handle the tracing bridge writes log lines into (see `main`), and
    /// `draft` the config already loaded there.
    pub fn new(cc: &eframe::CreationContext<'_>, shared: SharedHandle, draft: AgentConfig) -> Self {
        // The window now has room for an at-a-glance dashboard, so favor comfortably readable
        // desktop text over the compact defaults intended for small embedded panels.
        let mut style = (*cc.egui_ctx.style()).clone();
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(23.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(15.0));
        cc.egui_ctx.set_style(style);

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build the desktop agent's tokio runtime");
        let tray = AgentTray::new();
        if tray.is_none() {
            // no tray means no other way to reach the window, so keep it visible.
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        // mirrors the "Start agent" button's own gating: never auto-start into a config that can't
        // actually run (e.g. a first launch with no sandbox folder configured yet).
        if draft.auto_start && !draft.sandbox_root.trim().is_empty() {
            agent::start(rt.handle(), shared.clone(), draft.clone());
        }
        Self {
            rt,
            shared,
            draft,
            label_input: String::new(),
            reenrollment_token: String::new(),
            reenrollment_dialog: false,
            tray,
            last_tray_signature: None,
            log_filter: String::new(),
            follow_logs: true,
            quitting: false,
            exit_dialog: false,
            exit_dont_ask_again: false,
            started_at: Instant::now(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        let guard = self
            .shared
            .lock()
            .expect("desktop agent state lock poisoned");
        Snapshot {
            status: guard.status.clone(),
            connection: guard.connection.clone(),
            metrics: guard.metrics.clone(),
            busy: guard.busy,
            control: agent::control_state(&guard),
            agent_activity: guard.agent_activity.label.clone(),
            agent_activity_age: guard.agent_activity.since.elapsed(),
            worker_activity: guard.worker_activity.label.clone(),
            worker_activity_age: guard.worker_activity.since.elapsed(),
            resources: guard.resource_history.samples().cloned().collect(),
        }
    }

    // handle pending tray clicks/menu choices; called once per frame.
    fn handle_tray(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else {
            return;
        };
        match tray.poll() {
            Some(TrayAction::Open) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Some(TrayAction::OpenUi) => self.open_command_center(),
            Some(TrayAction::Exit) => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }

    // prefer launching the native app (if configured); fall back to the URL in the default browser.
    // failures just get logged, since there's no dialog surface here worth building for a one-off
    // "couldn't launch it" case.
    fn open_command_center(&self) {
        if let Err(err) = crate::launcher::open_command_center(
            &self.draft.command_center_app_path,
            &self.draft.command_center_url,
        ) {
            agent::log_line(
                &self.shared,
                format!("Failed to open command center: {err}"),
            );
        }
    }

    // Always ask before closing the main window: with a tray the agent may keep running unseen;
    // without one, the only available choice is a normal process exit.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if self.quitting {
            return;
        }
        let close_requested =
            ctx.input(|i| i.viewport().events.contains(&egui::ViewportEvent::Close));
        if close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            match self.draft.window_close_action {
                Some(WindowCloseAction::HideToTray) if self.tray.is_some() => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                Some(WindowCloseAction::Exit) => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {
                    self.exit_dont_ask_again = false;
                    self.exit_dialog = true;
                }
            }
        }
    }

    fn show_exit_dialog(&mut self, ctx: &egui::Context) {
        if !self.exit_dialog {
            return;
        }

        let can_hide = self.tray.is_some();
        let mut open = self.exit_dialog;
        let mut exit = false;
        let mut hide = false;
        egui::Window::new("Close Runinator Desktop Agent?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                if can_hide {
                    ui.label(
                        "The agent can keep running in the menu bar and continue accepting work.",
                    );
                } else {
                    ui.label("No system tray is available, so closing exits the agent.");
                }
                ui.add_space(10.0);
                ui.checkbox(&mut self.exit_dont_ask_again, "Don't ask again");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if can_hide && ui.button("Keep running in tray").clicked() {
                        hide = true;
                    }
                    if ui.button("Exit agent").clicked() {
                        exit = true;
                    }
                });
            });

        if hide {
            if self.exit_dont_ask_again {
                self.draft.window_close_action = Some(WindowCloseAction::HideToTray);
                config::save(&self.draft);
            }
            self.exit_dialog = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else if exit {
            if self.exit_dont_ask_again {
                self.draft.window_close_action = Some(WindowCloseAction::Exit);
                config::save(&self.draft);
            }
            self.exit_dialog = false;
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.exit_dialog = open;
        }
    }

    fn start_enrollment(&mut self) {
        let token = self.reenrollment_token.trim();
        if token.is_empty() || validate_config(&self.draft).is_some() {
            return;
        }

        let mut next_config = self.draft.clone();
        next_config.enrollment_token = Some(token.to_string());
        // Save the ordinary settings, but not the token: `enrollment_token` is skipped by serde
        // and `next_config` lives only long enough for the worker to redeem it.
        config::save(&self.draft);
        agent::start(self.rt.handle(), self.shared.clone(), next_config);
        self.reenrollment_token.clear();
        self.reenrollment_dialog = false;
    }

    fn show_reenrollment_dialog(&mut self, ctx: &egui::Context, reason: Option<&str>) {
        if !self.reenrollment_dialog {
            return;
        }

        let mut open = self.reenrollment_dialog;
        let mut re_enroll = false;
        let mut cancel = false;
        let can_open_ui = !self.draft.command_center_app_path.trim().is_empty()
            || !self.draft.command_center_url.trim().is_empty();
        let validation = validate_config(&self.draft);
        let re_enrolling = reason.is_some();
        let title = if re_enrolling {
            "Re-enroll desktop agent"
        } else {
            "Enroll desktop agent"
        };
        let action_label = if re_enrolling {
            "Re-enroll and start"
        } else {
            "Enroll and start"
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(if re_enrolling {
                    "The broker rejected this agent's saved credential. A new one-time enrollment token replaces it."
                } else {
                    "A one-time enrollment token securely creates this desktop agent's credential."
                });
                if let Some(reason) = reason {
                    ui.colored_label(DOT_RED, egui::RichText::new(reason).small());
                }
                ui.add_space(8.0);
                ui.label("1. In Command Center, open Replicas and choose Enroll a machine.");
                ui.label("2. Create and copy a new token, then paste it here.");
                ui.label(if re_enrolling {
                    "3. Re-enroll and start. The token is never saved in these settings."
                } else {
                    "3. Enroll and start. The token is never saved in these settings."
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new("One-time enrollment token").strong());
                ui.add(
                    egui::TextEdit::multiline(&mut self.reenrollment_token)
                        .password(true)
                        .desired_rows(3)
                        .desired_width(520.0)
                        .hint_text("Paste the token from Command Center"),
                );
                if let Some(reason) = validation.as_ref() {
                    ui.colored_label(DOT_RED, egui::RichText::new(reason).small());
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_open_ui, egui::Button::new("Open Command Center"))
                        .clicked()
                    {
                        self.open_command_center();
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    let ready = validation.is_none() && !self.reenrollment_token.trim().is_empty();
                    if ui
                        .add_enabled(ready, egui::Button::new(action_label))
                        .clicked()
                    {
                        re_enroll = true;
                    }
                });
            });

        if cancel {
            self.reenrollment_token.clear();
            self.reenrollment_dialog = false;
        } else {
            self.reenrollment_dialog = open;
        }
        if re_enroll {
            self.start_enrollment();
        }
    }

    // a tag-input for `draft.extra_labels`: fixed desktop identity labels are visible but cannot be
    // changed, while custom labels render as removable chips and accept the next `key=value` text.
    fn label_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(
                "Required labels identify this exclusive desktop worker. Add optional labels for \
                 more specific routing.",
            )
            .small()
            .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            for label in REQUIRED_LABELS {
                egui::Frame::default()
                    .fill(ui.visuals().selection.bg_fill)
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(format!("{label} (required)"));
                    });
            }
            let mut remove: Option<usize> = None;
            for (index, label) in self.draft.extra_labels.iter().enumerate() {
                egui::Frame::default()
                    .fill(ui.visuals().widgets.inactive.bg_fill)
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                    .show(ui, |ui: &mut egui::Ui| {
                        ui.label(label);
                        if ui.small_button("x").clicked() {
                            remove = Some(index);
                        }
                    });
            }
            if let Some(index) = remove {
                self.draft.extra_labels.remove(index);
            }

            let response = ui.add(
                egui::TextEdit::singleline(&mut self.label_input)
                    .hint_text("key=value")
                    .desired_width(140.0),
            );
            let committed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if committed {
                self.commit_label_input();
            }
        });

        let query = self.label_input.trim();
        if query.is_empty() {
            return;
        }
        let mut pick: Option<&'static str> = None;
        ui.horizontal_wrapped(|ui| {
            for suggestion in LABEL_SUGGESTIONS {
                let already_added = self
                    .draft
                    .extra_labels
                    .iter()
                    .any(|label| label == suggestion);
                if already_added || !suggestion.to_lowercase().starts_with(&query.to_lowercase()) {
                    continue;
                }
                if ui.small_button(*suggestion).clicked() {
                    pick = Some(suggestion);
                }
            }
        });
        if let Some(suggestion) = pick {
            self.label_input = suggestion.to_string();
            self.commit_label_input();
        }
    }

    // Parse `label_input` as a single custom `key=value` pair. Identity labels are installed by the
    // runtime and intentionally cannot be duplicated or overridden here.
    fn commit_label_input(&mut self) {
        let parsed = runinator_worker::parse_labels(Some(&self.label_input));
        let Some((key, value)) = parsed.into_iter().next() else {
            return;
        };
        let normalized = format!("{key}={value}");
        if config::is_reserved_identity_label(&normalized) {
            return;
        }
        if self.draft.extra_labels.contains(&normalized) {
            return;
        }
        self.draft.extra_labels.push(normalized);
        self.label_input.clear();
    }

    // push the current status to the tray only when it changed, so we don't rebuild the icon on every
    // repaint. a no-op when the platform tray failed to initialize.
    fn sync_tray(&mut self, presentation: &StatusPresentation) {
        if self.last_tray_signature.as_deref() == Some(presentation.tooltip.as_str()) {
            return;
        }
        if let Some(tray) = &self.tray {
            tray.set_status(presentation.tray_color, &presentation.tooltip);
        } else {
            return;
        }
        self.last_tray_signature = Some(presentation.tooltip.clone());
    }

    // a compact throughput readout for the running agent: in-flight vs. outcome totals, the latest
    // resource sample, and what this machine last executed.
    fn activity_panel(ui: &mut egui::Ui, metrics: &AgentMetrics) {
        let green = egui::Color32::from_rgb(64, 180, 96);
        let red = egui::Color32::from_rgb(210, 90, 70);
        let amber = egui::Color32::from_rgb(210, 170, 60);

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("In flight: {}", metrics.in_flight)).strong());
            ui.separator();
            ui.colored_label(green, format!("✓ {}", metrics.succeeded));
            ui.colored_label(red, format!("✗ {}", metrics.failed));
            ui.colored_label(amber, format!("⧖ {}", metrics.timed_out));
            if metrics.canceled > 0 {
                ui.label(format!("⊘ {}", metrics.canceled));
            }
        });

        if metrics.cpu_percent.is_some() || metrics.mem_percent.is_some() {
            let cpu = metrics
                .cpu_percent
                .map(|c| format!("CPU {c:.0}%"))
                .unwrap_or_default();
            let mem = metrics
                .mem_percent
                .map(|m| format!("RAM {m:.0}%"))
                .unwrap_or_default();
            ui.label(egui::RichText::new(format!("{cpu}   {mem}")).small().weak());
        }

        if let Some(last) = &metrics.last_completed {
            let (icon, color) = match last.outcome {
                ActionOutcome::Succeeded => ("✓", green),
                ActionOutcome::Failed => ("✗", red),
                ActionOutcome::TimedOut => ("⧖", amber),
                ActionOutcome::Canceled => ("⊘", egui::Color32::GRAY),
            };
            ui.colored_label(
                color,
                egui::RichText::new(format!(
                    "{icon} last: {} ({} ms)",
                    last.summary, last.duration_ms
                ))
                .small(),
            );
        }
    }

    fn runtime_lifecycle(ui: &mut egui::Ui, dashboard: &RuntimeDashboard<'_>) {
        ui.heading("Runtime");
        ui.label(
            egui::RichText::new(format!(
                "Running for {}",
                display_duration(dashboard.uptime)
            ))
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        ui.group(|ui| {
            ui.strong("Desktop agent");
            ui.label(format!(
                "Now: {} · for {}",
                dashboard.agent_activity,
                display_duration(dashboard.agent_activity_age)
            ));
            egui::Grid::new("runtime-agent-details")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    detail_row(ui, "Service", dashboard.service_url);
                    detail_row(
                        ui,
                        "Broker",
                        dashboard
                            .status
                            .broker_connection
                            .as_deref()
                            .unwrap_or("not connected"),
                    );
                    let replica = dashboard
                        .status
                        .replica_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "not registered".to_string());
                    detail_row(ui, "Replica", &replica);
                    detail_row(
                        ui,
                        "Sandbox",
                        dashboard.status.root.as_deref().unwrap_or("not configured"),
                    );
                });
        });
        ui.add_space(4.0);
        ui.group(|ui| {
            ui.strong("Worker");
            ui.label(format!(
                "Now: {} · for {}",
                dashboard.worker_activity,
                display_duration(dashboard.worker_activity_age)
            ));
            egui::Grid::new("runtime-worker-details")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    detail_row(ui, "Pool", "desktop (exclusive)");
                    detail_row(ui, "Action capacity", &dashboard.capacity.to_string());
                    detail_row(
                        ui,
                        "Available slots",
                        &dashboard
                            .capacity
                            .saturating_sub(dashboard.metrics.in_flight as usize)
                            .to_string(),
                    );
                });
            Self::activity_panel(ui, dashboard.metrics);
        });
    }

    /// Two columns on an ordinary desktop window; one when the window is narrowed, before either
    /// card can compete for width. Resource cards are deliberately a single column because their
    /// values are the least compressible text in the dashboard.
    fn runtime_dashboard(ui: &mut egui::Ui, dashboard: RuntimeDashboard<'_>) {
        ui.add_space(8.0);
        if ui.available_width() >= 1_000.0 {
            ui.columns(2, |columns| {
                Self::runtime_lifecycle(&mut columns[0], &dashboard);
                let telemetry = &mut columns[1];
                telemetry.heading("Resources");
                telemetry.label(egui::RichText::new("Last minute").small().weak());
                Self::resource_charts(telemetry, dashboard.resources);
            });
        } else {
            Self::runtime_lifecycle(ui, &dashboard);
            ui.add_space(8.0);
            ui.heading("Resources");
            ui.label(egui::RichText::new("Last minute").small().weak());
            Self::resource_charts(ui, dashboard.resources);
        }
    }

    fn settings_essentials(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.strong("Agent setup");
            egui::Grid::new("agent-config-form")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Service URL");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.service_url));
                    ui.end_row();

                    ui.label("Sandbox folder");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.sandbox_root)
                                .hint_text("/Users/me/runinator-files"),
                        );
                        if ui.button("Browse…").clicked()
                            && let Some(dir) = rfd::FileDialog::new().pick_folder()
                        {
                            self.draft.sandbox_root = dir.display().to_string();
                        }
                    });
                    ui.end_row();

                    ui.label("Working directory").on_hover_text(
                        "Base directory console.run commands execute from. Empty inherits this \
                         agent's own directory.",
                    );
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.console_working_dir)
                                .hint_text("optional — e.g. /Users/me/GitHub/runinator"),
                        );
                        if ui.button("Browse…").clicked()
                            && let Some(dir) = rfd::FileDialog::new().pick_folder()
                        {
                            self.draft.console_working_dir = dir.display().to_string();
                        }
                    });
                    ui.end_row();
                });
            ui.add_space(4.0);
            ui.checkbox(&mut self.draft.allow_write, "Allow writes and deletes")
                .on_hover_text("Off = read-only sandbox");
            ui.checkbox(&mut self.draft.auto_start, "Start automatically on launch");
        });
    }

    fn settings_connection(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.strong("Connection");
            egui::Grid::new("connection-form")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("API key");
                    ui.add(
                        egui::TextEdit::singleline(
                            self.draft.api_key.get_or_insert_with(String::new),
                        )
                        .password(true)
                        .hint_text("optional"),
                    );
                    ui.end_row();

                    ui.label("Broker");
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.draft.broker_mode,
                            crate::config::BrokerMode::Relay,
                            "Via web service",
                        );
                        ui.selectable_value(
                            &mut self.draft.broker_mode,
                            crate::config::BrokerMode::Direct,
                            "Direct",
                        )
                        .on_hover_text(
                            "Only use Direct when this machine is on the broker's trusted network.",
                        );
                    });
                    ui.end_row();

                    if self.draft.broker_mode == crate::config::BrokerMode::Direct {
                        ui.label("Backend");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.direct_broker_backend)
                                .hint_text("tcp | rabbitmq | kafka | http"),
                        );
                        ui.end_row();

                        ui.label("Endpoint");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.direct_broker_endpoint)
                                .hint_text("host:port, or amqp://user:pass@host:port/%2f"),
                        );
                        ui.end_row();
                    }
                });
        });
    }

    fn settings_desktop(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.strong("Desktop integration");
            egui::Grid::new("command-center-form")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Command center app");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft.command_center_app_path)
                            .hint_text("/Applications/Runinator Command Center.app"),
                    );
                    ui.end_row();

                    ui.label("Fallback URL");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft.command_center_url)
                            .hint_text("https://runinator.example.com/"),
                    );
                    ui.end_row();
                });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Routing labels").small().strong());
            self.label_editor(ui);
        });
    }

    fn settings_worker_tuning(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.strong("Worker limits");
            egui::Grid::new("worker-tuning-form")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Max concurrent actions");
                    ui.add(
                        egui::DragValue::new(&mut self.draft.max_concurrent_actions).range(1..=32),
                    );
                    ui.end_row();

                    ui.label("Shutdown grace (seconds)");
                    ui.add(
                        egui::DragValue::new(&mut self.draft.shutdown_grace_seconds).range(1..=300),
                    );
                    ui.end_row();

                    ui.label("Reconnect attempts").on_hover_text(
                        "Failed reconnects tolerated before the agent stops. 0 retries forever.",
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.draft.reconnect_max_attempts)
                            .range(0..=100)
                            .custom_formatter(|value, _| {
                                if value < 1.0 {
                                    "unlimited".to_string()
                                } else {
                                    format!("{value:.0}")
                                }
                            }),
                    );
                    ui.end_row();
                });
        });
    }

    fn settings_actions(&mut self, ui: &mut egui::Ui, starting: bool, busy: bool) {
        let validation = validate_config(&self.draft);
        ui.horizontal(|ui| {
            if starting {
                if ui
                    .button("Cancel startup")
                    .on_hover_text("Stop the agent coming up and return to settings")
                    .clicked()
                {
                    agent::cancel_start(self.rt.handle(), self.shared.clone());
                }
            } else {
                let can_start = !busy && validation.is_none();
                let start = ui.add_enabled(can_start, egui::Button::new("Start agent"));
                let start = match &validation {
                    Some(reason) => start.on_disabled_hover_text(reason.clone()),
                    None => start,
                };
                if start.clicked() {
                    config::save(&self.draft);
                    agent::start(self.rt.handle(), self.shared.clone(), self.draft.clone());
                }
            }

            let can_test = !busy && !self.draft.service_url.trim().is_empty();
            if ui
                .add_enabled(can_test, egui::Button::new("Test connection"))
                .on_hover_text("Check the service URL and API key without starting the agent")
                .clicked()
            {
                agent::test_connection(
                    self.rt.handle(),
                    self.shared.clone(),
                    self.draft.service_url.clone(),
                    self.draft.api_key.clone(),
                );
            }

            let can_enroll = !busy && validation.is_none();
            if ui
                .add_enabled(can_enroll, egui::Button::new("Enroll with token…"))
                .on_hover_text(
                    "Create a first-time desktop-agent credential with a one-time enrollment token",
                )
                .clicked()
            {
                self.reenrollment_dialog = true;
            }
        });
        if let Some(reason) = validation.filter(|_| !starting) {
            ui.colored_label(
                egui::Color32::from_rgb(210, 90, 70),
                egui::RichText::new(reason).small(),
            );
        }
    }

    /// Small native sparklines give the window the same short-horizon operational visibility as
    /// `--tui`, without adding a plotting dependency to the desktop agent.
    fn resource_charts(ui: &mut egui::Ui, samples: &[agent::ResourceSample]) {
        let latest = samples.last();
        let host_cpu = samples
            .iter()
            .map(|sample| sample.host_cpu_percent as f64)
            .collect::<Vec<_>>();
        let host_memory = samples
            .iter()
            .map(|sample| sample.host_mem_percent as f64)
            .collect::<Vec<_>>();
        let process_cpu = samples
            .iter()
            .map(|sample| sample.process_cpu_percent as f64)
            .collect::<Vec<_>>();
        let process_memory = samples
            .iter()
            .map(|sample| sample.process_mem_used_bytes as f64)
            .collect::<Vec<_>>();
        let network_rx = samples
            .iter()
            .map(|sample| sample.network_rx_bytes_per_sec)
            .collect::<Vec<_>>();
        let network_tx = samples
            .iter()
            .map(|sample| sample.network_tx_bytes_per_sec)
            .collect::<Vec<_>>();
        let disk_io = samples
            .iter()
            .map(|sample| sample.disk_io_bytes_per_sec)
            .collect::<Vec<_>>();

        resource_chart(
            ui,
            "Host CPU",
            latest
                .map(|sample| format!("{:.0}%", sample.host_cpu_percent))
                .unwrap_or_else(|| "collecting…".to_string()),
            &host_cpu,
            Some(100.0),
            DOT_BLUE,
        );
        resource_chart(
            ui,
            "Host RAM",
            latest
                .map(|sample| {
                    format!(
                        "{:.0}% · {}/{}",
                        sample.host_mem_percent,
                        format_bytes(sample.host_mem_used_bytes as f64),
                        format_bytes(sample.host_mem_total_bytes as f64)
                    )
                })
                .unwrap_or_else(|| "collecting…".to_string()),
            &host_memory,
            Some(100.0),
            DOT_GREEN,
        );
        resource_chart(
            ui,
            "Process CPU",
            latest
                .map(|sample| format!("{:.0}%", sample.process_cpu_percent))
                .unwrap_or_else(|| "collecting…".to_string()),
            &process_cpu,
            None,
            DOT_AMBER,
        );
        resource_chart(
            ui,
            "Process RAM",
            latest
                .map(|sample| format_bytes(sample.process_mem_used_bytes as f64))
                .unwrap_or_else(|| "collecting…".to_string()),
            &process_memory,
            None,
            DOT_GREEN,
        );
        resource_chart(
            ui,
            "Network RX",
            latest
                .map(|sample| format!("{}/s", format_bytes(sample.network_rx_bytes_per_sec)))
                .unwrap_or_else(|| "collecting…".to_string()),
            &network_rx,
            None,
            DOT_BLUE,
        );
        resource_chart(
            ui,
            "Network TX",
            latest
                .map(|sample| format!("{}/s", format_bytes(sample.network_tx_bytes_per_sec)))
                .unwrap_or_else(|| "collecting…".to_string()),
            &network_tx,
            None,
            DOT_BLUE,
        );
        resource_chart(
            ui,
            "Disk I/O",
            latest
                .map(|sample| format!("{}/s", format_bytes(sample.disk_io_bytes_per_sec)))
                .unwrap_or_else(|| "collecting…".to_string()),
            &disk_io,
            None,
            DOT_AMBER,
        );
    }

    // the log console, rendered in a bottom-pinned panel so it stays at the foot of the window while
    // the config/status area scrolls above it. carries the level/filter controls, copy/save/clear
    // actions, and the (filtered) line view.
    fn log_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Log");
            ui.label("Level");
            egui::ComboBox::from_id_salt("log-level")
                .selected_text(self.draft.log_level.as_str())
                .show_ui(ui, |ui| {
                    for level in LogLevel::ALL {
                        if ui
                            .selectable_value(&mut self.draft.log_level, level, level.as_str())
                            .changed()
                        {
                            // apply live to the running subscriber and persist for next launch.
                            logging::set_level(level);
                            config::save(&self.draft);
                        }
                    }
                });

            ui.separator();
            ui.checkbox(&mut self.follow_logs, "Follow live")
                .on_hover_text("Roll new log entries into view as they arrive");
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.log_filter)
                    .hint_text("filter")
                    .desired_width(120.0),
            );
            if !self.log_filter.is_empty() && ui.small_button("✕").clicked() {
                self.log_filter.clear();
            }
        });

        // snapshot the (filtered) lines under one short lock, then release it before rendering or
        // writing to the clipboard/disk.
        let filter = self.log_filter.trim().to_lowercase();
        let lines: Vec<String> = {
            let guard = self
                .shared
                .lock()
                .expect("desktop agent state lock poisoned");
            guard
                .logs
                .iter()
                .filter(|line| filter.is_empty() || line.to_lowercase().contains(&filter))
                .cloned()
                .collect()
        };

        ui.horizontal(|ui| {
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(lines.join("\n"));
            }
            if ui.button("Save…").clicked() {
                self.save_log(&lines);
            }
            if ui.button("Clear").clicked() {
                self.shared
                    .lock()
                    .expect("desktop agent state lock poisoned")
                    .logs
                    .clear();
            }
            ui.label(
                egui::RichText::new(if filter.is_empty() {
                    format!("{} lines", lines.len())
                } else {
                    format!("{} matching lines", lines.len())
                })
                .small()
                .weak(),
            );
        });

        let follow_logs = self.follow_logs;
        egui::ScrollArea::vertical()
            .animated(true)
            .stick_to_bottom(follow_logs)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &lines {
                    ui.monospace(line);
                }
                if follow_logs {
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                }
            });
    }

    // write the currently shown (filtered) log lines to a file the operator picks; outcome is logged
    // back into the console rather than surfaced in a dialog.
    fn save_log(&self, lines: &[String]) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("runinator-desktop-agent.log")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, lines.join("\n")) {
            Ok(()) => agent::log_line(&self.shared, format!("Saved log to {}", path.display())),
            Err(err) => agent::log_line(&self.shared, format!("Failed to save log: {err}")),
        }
    }
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).strong());
    ui.label(value);
    ui.end_row();
}

fn resource_chart(
    ui: &mut egui::Ui,
    title: &str,
    value: String,
    values: &[f64],
    fixed_max: Option<f64>,
    color: egui::Color32,
) {
    ui.group(|ui| {
        // Keep the label and its value on independent lines. Long values (notably total RAM and
        // broker-derived units) can then wrap naturally instead of colliding in a narrow card.
        ui.label(egui::RichText::new(title).small().strong());
        ui.label(egui::RichText::new(value).small().weak());
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::hover());
        let values = values
            .iter()
            .copied()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .collect::<Vec<_>>();
        if values.len() < 2 || rect.width() <= 1.0 {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "collecting…",
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
            return;
        }
        let max = fixed_max
            .unwrap_or_else(|| values.iter().copied().fold(0.0_f64, f64::max))
            .max(1.0);
        let points = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let x = rect.left() + rect.width() * index as f32 / (values.len() - 1) as f32;
                let y = rect.bottom() - rect.height() * (*value / max).clamp(0.0, 1.0) as f32;
                egui::pos2(x, y)
            })
            .collect::<Vec<_>>();
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0_f32, DOT_GRAY),
        );
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
    });
}

/// A painted light, rather than a font-dependent Unicode glyph, makes the current lifecycle state
/// readable at a glance on every platform and at every UI scale.
fn status_light(ui: &mut egui::Ui, presentation: &StatusPresentation) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let center = rect.center();
    ui.painter().circle_filled(center, 5.5, presentation.color);
    ui.painter().circle_stroke(
        center,
        5.5,
        egui::Stroke::new(1.0_f32, ui.visuals().widgets.inactive.bg_fill),
    );
    response.on_hover_text(&presentation.tooltip);
}

fn display_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3_600 {
        format!("{}h {:02}m", seconds / 3_600, (seconds % 3_600) / 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn format_bytes(value: f64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = value.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

impl eframe::App for DesktopAgentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // the worker loop runs on a background runtime, so poll for its status/log updates, and for
        // tray clicks, on a timer rather than only on window events (the window may be hidden).
        ctx.request_repaint_after(Duration::from_millis(400));

        self.handle_tray(ctx);
        self.handle_close_request(ctx);
        self.show_exit_dialog(ctx);

        let Snapshot {
            status,
            connection,
            metrics,
            busy,
            control,
            agent_activity,
            agent_activity_age,
            worker_activity,
            worker_activity_age,
            resources,
        } = self.snapshot();

        let reenrollment_reason = match &connection {
            ConnectionState::ReenrollmentRequired { reason } => Some(reason.clone()),
            _ => None,
        };
        self.show_reenrollment_dialog(ctx, reenrollment_reason.as_deref());

        let presentation = present_status(&connection, busy);
        self.sync_tray(&presentation);

        // pin the log to the bottom of the window (added before the central panel, per egui's panel
        // ordering) so it stays put while the config/status area above it scrolls.
        egui::TopBottomPanel::bottom("log-panel")
            .resizable(true)
            .default_height(280.0)
            .show(ctx, |ui| {
                self.log_panel(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let has_app = !self.draft.command_center_app_path.trim().is_empty();
                    let has_url = !self.draft.command_center_url.trim().is_empty();
                    ui.horizontal(|ui| {
                        ui.heading("Runinator Desktop Agent");
                        status_light(ui, &presentation);
                        ui.colored_label(presentation.color, &presentation.label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if status.running
                                && ui
                                    .add_enabled(!busy, egui::Button::new("Stop agent"))
                                    .clicked()
                            {
                                agent::stop(self.rt.handle(), self.shared.clone());
                            }
                            if ui
                                .add_enabled(has_app || has_url, egui::Button::new("Open UI"))
                                .on_hover_text(if has_app {
                                    "Launch the command-center app"
                                } else {
                                    "Open the command center in your default browser"
                                })
                                .clicked()
                            {
                                self.open_command_center();
                            }
                        });
                    });
                    match &connection {
                        ConnectionState::Reconnecting {
                            retry_secs,
                            attempt,
                            max_attempts,
                        } => {
                            let budget = match max_attempts {
                                Some(max) => format!(" (attempt {attempt} of {max})"),
                                None => String::new(),
                            };
                            ui.colored_label(
                                presentation.color,
                                egui::RichText::new(format!(
                                    "Broker unreachable — retrying in {retry_secs}s{budget}"
                                ))
                                .small(),
                            );
                        }
                        // the agent stopped itself here, so say what ends the state: the operator has to
                        // start it again once the service is back.
                        ConnectionState::Disconnected { attempts, reason } => {
                            ui.colored_label(
                        presentation.color,
                        egui::RichText::new(format!(
                            "Disconnected after {attempts} reconnect attempts ({reason}). The \
                             agent stopped; press Start agent to try again."
                        ))
                        .small(),
                    );
                        }
                        ConnectionState::ReenrollmentRequired { reason } => {
                            ui.group(|ui| {
                                ui.colored_label(
                                    DOT_RED,
                                    egui::RichText::new("Broker credential rejected").strong(),
                                );
                                ui.label(
                                    "This agent stopped to avoid retrying an invalid credential. Re-enroll it with a new one-time token.",
                                );
                                ui.label(egui::RichText::new(reason).small().weak());
                                if ui.button("Re-enroll desktop agent…").clicked() {
                                    self.reenrollment_dialog = true;
                                }
                            });
                        }
                        _ => {}
                    }
                    ui.separator();

                    if status.running {
                        Self::runtime_dashboard(
                            ui,
                            RuntimeDashboard {
                                status: &status,
                                metrics: &metrics,
                                agent_activity: &agent_activity,
                                agent_activity_age,
                                worker_activity: &worker_activity,
                                worker_activity_age,
                                resources: &resources,
                                uptime: self.started_at.elapsed(),
                                capacity: self.draft.max_concurrent_actions.max(1),
                                service_url: &self.draft.service_url,
                            },
                        );
                    } else {
                        let starting = control == Control::Starting;
                        if starting {
                            // the form stays editable while a start is in flight, so say what an edit does
                            // now: the running attempt is already carrying the config it was given.
                            ui.colored_label(
                                DOT_BLUE,
                                egui::RichText::new(
                                    "Starting — changes below apply the next time you start.",
                                )
                                .small(),
                            );
                            ui.add_space(4.0);
                        }
                        if self.draft.api_key.as_deref().is_some_and(str::is_empty) {
                            self.draft.api_key = None;
                        }

                        if ui.available_width() >= 1_000.0 {
                            ui.columns(2, |columns| {
                                self.settings_essentials(&mut columns[0]);
                                self.settings_connection(&mut columns[1]);
                                columns[0].add_space(8.0);
                                self.settings_actions(&mut columns[0], starting, busy);
                                columns[1].add_space(8.0);
                                self.settings_desktop(&mut columns[1]);
                                columns[1].add_space(8.0);
                                self.settings_worker_tuning(&mut columns[1]);
                            });
                        } else {
                            self.settings_essentials(ui);
                            ui.add_space(8.0);
                            self.settings_connection(ui);
                            ui.add_space(8.0);
                            self.settings_desktop(ui);
                            ui.add_space(8.0);
                            self.settings_worker_tuning(ui);
                            ui.add_space(8.0);
                            self.settings_actions(ui, starting, busy);
                        }
                    }
                });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Do this before `DesktopAgentApp` drops its runtime. `AgentHandle` detaches on drop, so
        // merely asking the agent to stop from the tray handler can otherwise leave Cargo waiting
        // on work that outlived the eframe window.
        agent::shutdown_for_process_exit(&self.rt, &self.shared);
    }
}

#[cfg(test)]
#[path = "gui_tests.rs"]
mod tests;
