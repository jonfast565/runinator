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

pub struct DesktopAgentApp {
    rt: tokio::runtime::Runtime,
    shared: SharedHandle,
    // the editable draft bound to the form; only applied to the running agent on "Start".
    draft: AgentConfig,
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
        Self {
            rt,
            shared: Arc::new(Mutex::new(Shared::default())),
            draft: config::load(),
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
            Some(TrayAction::Exit) => {
                self.quitting = true;
                agent::stop(self.rt.handle(), self.shared.clone());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
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
                "Runs this machine as a sandboxed local-files worker for Runinator workflows.",
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

            if status.running {
                ui.add_space(4.0);
                if let Some(replica_id) = status.replica_id {
                    ui.label(format!("Replica: {replica_id}"));
                }
                ui.label(format!("Root: {}", status.root.clone().unwrap_or_default()));
                ui.label(format!(
                    "Broker: {}",
                    status.broker_url.clone().unwrap_or_default()
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

                        ui.label("Broker URL");
                        ui.add(egui::TextEdit::singleline(&mut self.draft.broker_url));
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
                    });

                if self.draft.api_key.as_deref().is_some_and(str::is_empty) {
                    self.draft.api_key = None;
                }

                ui.checkbox(&mut self.draft.allow_write, "Allow writes and deletes");
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
