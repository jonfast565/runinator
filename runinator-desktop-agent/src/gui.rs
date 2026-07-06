//! the desktop agent's control surface: a small window to configure the sandbox folder, start/stop
//! the worker loop, and watch its status. it starts hidden behind a tray icon (see [`crate::tray`]);
//! the window's close button only hides it again, so "Exit" from the tray menu is the one real quit
//! path. deliberately minimal — this is a status console for the agent process, not a workflow editor.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::agent::{self, AgentConfig, AgentStatus, Shared, SharedHandle};
use crate::config;
use crate::tray::{AgentTray, TrayAction};

// presets offered by the label type-ahead. not exhaustive — any `key=value` text the operator types
// is accepted; this just surfaces labels a pack in this repo is already known to route on.
const LABEL_SUGGESTIONS: &[&str] = &["runner=creds-sync", "pool=desktop"];

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
    // set once "Exit" is chosen, so the window's own close-intercept doesn't cancel our own Close cmd.
    quitting: bool,
}

impl DesktopAgentApp {
    /// builds the app, including the tray icon. must run on the main thread after the platform event
    /// loop has started — `cc` (handed to eframe's app-creator closure) guarantees that timing.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        let shared = Arc::new(Mutex::new(Shared::default()));
        let draft = config::load();
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
            quitting: false,
        }
    }

    fn snapshot(&self) -> (AgentStatus, bool) {
        let guard = self
            .shared
            .lock()
            .expect("desktop agent state lock poisoned");
        (guard.status.clone(), guard.busy)
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
}

impl eframe::App for DesktopAgentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // the worker loop runs on a background runtime, so poll for its status/log updates, and for
        // tray clicks, on a timer rather than only on window events (the window may be hidden).
        ctx.request_repaint_after(Duration::from_millis(400));

        self.handle_tray(ctx);
        self.handle_close_request(ctx);

        let (status, busy) = self.snapshot();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Runinator Desktop Agent");
            ui.label(
                "Runs this machine as a worker for Runinator workflows: a sandboxed local-files \
                 provider plus every built-in provider, gated to only the work explicitly pinned \
                 or labeled to it.",
            );
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Status:");
                let (text, color) = if status.running {
                    ("running", egui::Color32::from_rgb(64, 180, 96))
                } else if busy {
                    ("working…", egui::Color32::from_rgb(210, 170, 60))
                } else {
                    ("stopped", egui::Color32::GRAY)
                };
                ui.colored_label(color, text);
            });

            ui.add_space(4.0);
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
                ui.add_space(4.0);
                if let Some(replica_id) = status.replica_id {
                    ui.label(format!("Replica: {replica_id}"));
                }
                ui.label(format!("Root: {}", status.root.clone().unwrap_or_default()));
                ui.label(format!(
                    "Broker: {}",
                    status.broker_connection.clone().unwrap_or_default()
                ));
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

                        ui.label("Command Center App");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.command_center_app_path)
                                .hint_text("/Applications/Runinator Command Center.app"),
                        );
                        ui.end_row();

                        ui.label("Command Center URL");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.command_center_url)
                                .hint_text("https://runinator.example.com/ (fallback if no app)"),
                        );
                        ui.end_row();

                        ui.label("Sandbox folder");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.draft.sandbox_root)
                                .hint_text("/Users/me/runinator-files"),
                        );
                        ui.end_row();

                        ui.label("API key");
                        ui.add(
                            egui::TextEdit::singleline(
                                self.draft.api_key.get_or_insert_with(String::new),
                            )
                            .password(true)
                            .hint_text("optional"),
                        );
                        ui.end_row();

                        ui.label("Broker connection");
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.draft.broker_mode,
                                crate::config::BrokerMode::Relay,
                                "Via web service (relay)",
                            );
                            ui.selectable_value(
                                &mut self.draft.broker_mode,
                                crate::config::BrokerMode::Direct,
                                "Direct",
                            );
                        });
                        ui.end_row();

                        if self.draft.broker_mode == crate::config::BrokerMode::Direct {
                            ui.label("Broker backend");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.draft.direct_broker_backend)
                                    .hint_text("tcp | rabbitmq | kafka | http"),
                            );
                            ui.end_row();

                            ui.label("Broker endpoint");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.draft.direct_broker_endpoint)
                                    .hint_text("host:port, or amqp://user:pass@host:port/%2f"),
                            );
                            ui.end_row();
                        }
                    });

                if self.draft.broker_mode == crate::config::BrokerMode::Direct {
                    ui.label(
                        egui::RichText::new(
                            "Connects straight to the broker instead of relaying through the web \
                             service. Only do this if this machine is actually on the broker's \
                             trusted network — otherwise leave it on \"Via web service\".",
                        )
                        .small()
                        .weak(),
                    );
                }

                if self.draft.api_key.as_deref().is_some_and(str::is_empty) {
                    self.draft.api_key = None;
                }

                ui.add_space(6.0);
                ui.label("Extra labels");
                ui.label(
                    egui::RichText::new(
                        "Beyond the always-on pool=desktop. A workflow node with a matching \
                         .runner(\"...\") or label requirement (e.g. packs/creds-sync's \
                         runner=creds-sync) routes to this machine; nothing else changes on the \
                         agent side per label.",
                    )
                    .small()
                    .weak(),
                );
                self.label_editor(ui);
                ui.add_space(4.0);

                ui.checkbox(&mut self.draft.allow_write, "Allow writes and deletes");
                ui.checkbox(
                    &mut self.draft.auto_start,
                    "Start automatically when this app launches",
                );
                ui.add_space(8.0);

                let can_start = !busy && !self.draft.sandbox_root.trim().is_empty();
                if ui
                    .add_enabled(can_start, egui::Button::new("Start agent"))
                    .clicked()
                {
                    config::save(&self.draft);
                    agent::start(self.rt.handle(), self.shared.clone(), self.draft.clone());
                }
            }

            ui.separator();
            ui.label("Log");
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height(200.0)
                .show(ui, |ui| {
                    let guard = self
                        .shared
                        .lock()
                        .expect("desktop agent state lock poisoned");
                    for line in guard.logs.iter() {
                        ui.monospace(line);
                    }
                });
        });
    }
}
