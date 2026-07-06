//! standalone desktop worker with a small egui control panel. it derives its runtime behavior from
//! `runinator-worker` (same `WorkerRuntime`/`start_worker_loop` the server-side worker binary uses)
//! but only ever runs the local-files provider against a user-chosen sandbox folder, registered as an
//! exclusive `desktop` replica. replaces the worker that used to be embedded in the Tauri command
//! center: that app now only talks to a Runinator service, it does not execute actions itself.
//!
//! the process lives in the tray by default (see [`tray`]): the window starts hidden and only opens
//! when the tray icon is clicked, so running the agent doesn't clutter the dock/taskbar.

mod agent;
mod config;
mod gui;
mod launcher;
mod tray;

fn main() -> eframe::Result<()> {
    env_logger::init();

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([440.0, 560.0])
            .with_visible(false),
        ..Default::default()
    };

    eframe::run_native(
        "Runinator Desktop Agent",
        native_options,
        Box::new(|cc| Ok(Box::new(gui::DesktopAgentApp::new(cc)))),
    )
}
