//! the desktop agent's control surface: a small window to configure the sandbox folder, start/stop
//! the worker loop, and watch its status. it starts hidden behind a tray icon (see [`crate::tray`]);
//! the window's close button only hides it again, so "Exit" from the tray menu is the one real quit
//! path. deliberately minimal — this is a status console for the agent process, not a workflow editor.

use std::time::Duration;

use eframe::egui;

use crate::agent::{self, AgentConfig, AgentMetrics, AgentStatus, ConnectionState, SharedHandle};
use crate::config::{self, LogLevel};
use crate::logging;
use crate::tray::{AgentTray, TrayAction, TrayColor};
use runinator_worker::ActionOutcome;

// presets offered by the label type-ahead. not exhaustive — any `key=value` text the operator types
// is accepted; this just surfaces labels a pack in this repo is already known to route on.
const LABEL_SUGGESTIONS: &[&str] = &["runner=creds-sync", "pool=desktop"];

/// a per-frame copy of the shared agent state the GUI renders from, taken under one short lock.
struct Snapshot {
    status: AgentStatus,
    connection: ConnectionState,
    metrics: AgentMetrics,
    busy: bool,
}

/// how a connection state renders: header dot text + color, and the matching tray color/tooltip.
struct StatusPresentation {
    label: &'static str,
    color: egui::Color32,
    tray_color: TrayColor,
    tooltip: String,
}

fn present_status(connection: &ConnectionState, busy: bool) -> StatusPresentation {
    // a start/stop transition in flight reads as "working" regardless of the underlying phase.
    if busy {
        return StatusPresentation {
            label: "● working…",
            color: egui::Color32::from_rgb(210, 170, 60),
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — working…".to_string(),
        };
    }
    match connection {
        ConnectionState::Stopped => StatusPresentation {
            label: "● stopped",
            color: egui::Color32::GRAY,
            tray_color: TrayColor::Idle,
            tooltip: "Runinator Desktop Agent — stopped".to_string(),
        },
        ConnectionState::Connecting => StatusPresentation {
            label: "● connecting…",
            color: egui::Color32::from_rgb(45, 140, 200),
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — connecting…".to_string(),
        },
        ConnectionState::Connected => StatusPresentation {
            label: "● running",
            color: egui::Color32::from_rgb(64, 180, 96),
            tray_color: TrayColor::Connected,
            tooltip: "Runinator Desktop Agent — running".to_string(),
        },
        ConnectionState::Reconnecting { retry_secs } => StatusPresentation {
            label: "● reconnecting",
            color: egui::Color32::from_rgb(210, 90, 70),
            tray_color: TrayColor::Degraded,
            tooltip: format!("Runinator Desktop Agent — reconnecting (retry in {retry_secs}s)"),
        },
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
    // `None` when the platform tray failed to initialize; the window is then the only way in, so it
    // starts visible rather than stranding the user with no way to reach it.
    tray: Option<AgentTray>,
    // last tray icon/tooltip pushed, so we only touch the platform tray when the state actually
    // changes rather than on every 400ms repaint.
    last_tray_signature: Option<String>,
    // case-insensitive substring filter applied to the log console; empty shows everything.
    log_filter: String,
    // set once "Exit" is chosen, so the window's own close-intercept doesn't cancel our own Close cmd.
    quitting: bool,
}

impl DesktopAgentApp {
    /// builds the app, including the tray icon. must run on the main thread after the platform event
    /// loop has started — `cc` (handed to eframe's app-creator closure) guarantees that timing.
    /// `shared` is the same state handle the tracing bridge writes log lines into (see `main`), and
    /// `draft` the config already loaded there.
    pub fn new(cc: &eframe::CreationContext<'_>, shared: SharedHandle, draft: AgentConfig) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build the desktop agent's tokio runtime");
        let tray = AgentTray::new();
        if tray.is_none() {
            // no tray means no other way to reach the window, so don't start hidden.
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
            tray,
            last_tray_signature: None,
            log_filter: String::new(),
            quitting: false,
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
                agent::stop(self.rt.handle(), self.shared.clone());
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

    // a tray icon means the window's close button should hide it, not end the process; without a
    // tray there is no other way back in, so let the close button behave normally.
    fn handle_close_request(&self, ctx: &egui::Context) {
        if self.quitting || self.tray.is_none() {
            return;
        }
        let close_requested =
            ctx.input(|i| i.viewport().events.contains(&egui::ViewportEvent::Close));
        if close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    // a tag-input for `draft.extra_labels`: existing labels render as removable chips, a text field
    // takes the next `key=value` (committed on Enter or by picking a type-ahead suggestion below it).
    fn label_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Route work to this machine with a matching .runner(\"...\").")
                .small()
                .weak(),
        );
        ui.horizontal_wrapped(|ui| {
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

    // parse `label_input` as a single `key=value` pair and, if valid and not a duplicate, push it
    // onto `draft.extra_labels` and clear the input. invalid or duplicate text is left in the field
    // uncommitted rather than silently dropped, so the operator can see and fix it.
    fn commit_label_input(&mut self) {
        let parsed = runinator_worker::parse_labels(Some(&self.label_input));
        let Some((key, value)) = parsed.into_iter().next() else {
            return;
        };
        let normalized = format!("{key}={value}");
        if self
            .draft
            .extra_labels
            .iter()
            .any(|label| *label == normalized)
        {
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

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &lines {
                    ui.monospace(line);
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

impl eframe::App for DesktopAgentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // the worker loop runs on a background runtime, so poll for its status/log updates, and for
        // tray clicks, on a timer rather than only on window events (the window may be hidden).
        ctx.request_repaint_after(Duration::from_millis(400));

        self.handle_tray(ctx);
        self.handle_close_request(ctx);

        let Snapshot {
            status,
            connection,
            metrics,
            busy,
        } = self.snapshot();

        let presentation = present_status(&connection, busy);
        self.sync_tray(&presentation);

        // pin the log to the bottom of the window (added before the central panel, per egui's panel
        // ordering) so it stays put while the config/status area above it scrolls.
        egui::TopBottomPanel::bottom("log-panel")
            .resizable(true)
            .default_height(200.0)
            .show(ctx, |ui| {
                self.log_panel(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Runinator Desktop Agent");
                ui.colored_label(presentation.color, presentation.label);
            });
            if let ConnectionState::Reconnecting { retry_secs } = connection {
                ui.colored_label(
                    presentation.color,
                    egui::RichText::new(format!("Broker unreachable — retrying in {retry_secs}s"))
                        .small(),
                );
            }
            ui.separator();

            let has_app = !self.draft.command_center_app_path.trim().is_empty();
            let has_url = !self.draft.command_center_url.trim().is_empty();
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

            if status.running {
                ui.add_space(6.0);
                if let Some(replica_id) = status.replica_id {
                    ui.label(format!("Replica: {replica_id}"));
                }
                ui.label(format!("Root: {}", status.root.clone().unwrap_or_default()));
                ui.label(format!(
                    "Broker: {}",
                    status.broker_connection.clone().unwrap_or_default()
                ));

                Self::activity_panel(ui, &metrics);

                ui.add_space(8.0);
                if ui
                    .add_enabled(!busy, egui::Button::new("Stop agent"))
                    .clicked()
                {
                    agent::stop(self.rt.handle(), self.shared.clone());
                }
            } else {
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
                            if ui.button("Browse…").clicked() {
                                // a native modal folder picker; declining it leaves the field as-is.
                                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                    self.draft.sandbox_root = dir.display().to_string();
                                }
                            }
                        });
                        ui.end_row();

                        ui.label("Working directory").on_hover_text(
                            "Base directory console.run commands execute from, so a workflow can \
                                 reference files by a relative path (e.g. a repo checkout for \
                                 packs/creds-sync). Empty inherits this agent's own directory.",
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.draft.console_working_dir)
                                    .hint_text("optional — e.g. /Users/me/GitHub/runinator"),
                            );
                            if ui.button("Browse…").clicked() {
                                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                    self.draft.console_working_dir = dir.display().to_string();
                                }
                            }
                        });
                        ui.end_row();
                    });

                ui.add_space(4.0);
                ui.checkbox(&mut self.draft.allow_write, "Allow writes and deletes")
                    .on_hover_text("Off = read-only sandbox");
                ui.checkbox(&mut self.draft.auto_start, "Start automatically on launch");

                if self.draft.api_key.as_deref().is_some_and(str::is_empty) {
                    self.draft.api_key = None;
                }

                ui.add_space(8.0);
                egui::CollapsingHeader::new("Command center")
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::Grid::new("command-center-form")
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("App");
                                ui.add(
                                    egui::TextEdit::singleline(
                                        &mut self.draft.command_center_app_path,
                                    )
                                    .hint_text("/Applications/Runinator Command Center.app"),
                                );
                                ui.end_row();

                                ui.label("URL");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.draft.command_center_url)
                                        .hint_text("https://runinator.example.com/ (fallback)"),
                                );
                                ui.end_row();
                            });
                    });

                egui::CollapsingHeader::new("Connection")
                    .default_open(false)
                    .show(ui, |ui| {
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
                                        "Only if this machine is on the broker's trusted \
                                         network; otherwise leave it on \"Via web service\".",
                                    );
                                });
                                ui.end_row();

                                if self.draft.broker_mode == crate::config::BrokerMode::Direct {
                                    ui.label("Backend");
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut self.draft.direct_broker_backend,
                                        )
                                        .hint_text("tcp | rabbitmq | kafka | http"),
                                    );
                                    ui.end_row();

                                    ui.label("Endpoint");
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut self.draft.direct_broker_endpoint,
                                        )
                                        .hint_text("host:port, or amqp://user:pass@host:port/%2f"),
                                    );
                                    ui.end_row();
                                }
                            });
                    });

                egui::CollapsingHeader::new("Labels")
                    .default_open(false)
                    .show(ui, |ui| self.label_editor(ui));

                egui::CollapsingHeader::new("Worker tuning")
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::Grid::new("worker-tuning-form")
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("Max concurrent actions");
                                ui.add(
                                    egui::DragValue::new(&mut self.draft.max_concurrent_actions)
                                        .range(1..=32),
                                );
                                ui.end_row();

                                ui.label("Shutdown grace (seconds)");
                                ui.add(
                                    egui::DragValue::new(&mut self.draft.shutdown_grace_seconds)
                                        .range(1..=300),
                                );
                                ui.end_row();
                            });
                    });

                ui.add_space(8.0);
                let validation = validate_config(&self.draft);
                ui.horizontal(|ui| {
                    let can_start = !busy && validation.is_none();
                    let start = ui.add_enabled(can_start, egui::Button::new("Start agent"));
                    // surface why Start is blocked on hover rather than leaving a dead button.
                    let start = match &validation {
                        Some(reason) => start.on_disabled_hover_text(reason.clone()),
                        None => start,
                    };
                    if start.clicked() {
                        config::save(&self.draft);
                        agent::start(self.rt.handle(), self.shared.clone(), self.draft.clone());
                    }

                    // a throwaway connectivity probe; independent of the sandbox/broker config, so
                    // it only needs a service URL.
                    let can_test = !busy && !self.draft.service_url.trim().is_empty();
                    if ui
                        .add_enabled(can_test, egui::Button::new("Test connection"))
                        .on_hover_text(
                            "Check the service URL and API key without starting the agent",
                        )
                        .clicked()
                    {
                        agent::test_connection(
                            self.rt.handle(),
                            self.shared.clone(),
                            self.draft.service_url.clone(),
                            self.draft.api_key.clone(),
                        );
                    }
                });
                if let Some(reason) = &validation {
                    ui.colored_label(
                        egui::Color32::from_rgb(210, 90, 70),
                        egui::RichText::new(reason).small(),
                    );
                }
            }
        });
    }
}
