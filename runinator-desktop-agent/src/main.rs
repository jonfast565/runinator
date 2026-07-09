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
mod logging;
mod notify;
mod single_instance;
mod tray;

use std::sync::{Arc, Mutex};

fn main() -> eframe::Result<()> {
    // ensure only one desktop agent runs at a time: two copies would both register the exclusive
    // `desktop` replica and contend for the same pinned/labeled work. a second launch surfaces a
    // dialog and exits instead of starting a rival worker loop.
    let _instance = match single_instance::acquire() {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            single_instance::warn_already_running();
            return Ok(());
        }
        // an unexpected bind failure must not lock the operator out of their own agent; note it and
        // start anyway rather than refusing to run.
        Err(err) => {
            eprintln!("desktop-agent single-instance check failed, starting anyway: {err}");
            None
        }
    };

    // load config up front so the log console starts at the persisted level, and share one state
    // handle between the tracing bridge (which writes log lines into it) and the GUI (which reads them).
    let draft = config::load();
    let shared = Arc::new(Mutex::new(agent::Shared::default()));
    logging::init(shared.clone(), draft.log_level);

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([440.0, 560.0])
            .with_visible(true),
        ..Default::default()
    };

    eframe::run_native(
        "Runinator Desktop Agent",
        native_options,
        Box::new(move |cc| Ok(Box::new(gui::DesktopAgentApp::new(cc, shared, draft)))),
    )
}
