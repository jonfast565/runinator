//! standalone worker for this machine, in either of two shapes: a tray/window app, or (with
//! `--headless`) a background service.
//!
//! both host the same lifecycle — `runinator_worker::agent::AgentRuntime`, the very code the
//! server-side `runinator-worker` binary runs — registered as an exclusive `pool=desktop` replica
//! carrying the built-in provider catalog plus the sandboxed local-files provider. the only thing
//! the gui adds is an observer: a console, a status header, and native toasts.
//!
//! closing the control window hides it in the tray (see [`tray`]); the tray's Exit action is the
//! explicit process shutdown path.

mod agent;
mod cli;
mod config;
mod errors;
mod gui;
mod headless;
mod launcher;
mod logging;
mod notify;
mod service;
mod single_instance;
mod tray;
mod tui;

use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::Parser;

use crate::cli::CliArgs;
use crate::config::AgentConfig;

fn main() -> ExitCode {
    let args = CliArgs::parse();
    // Precedence is CLI > env > saved JSON > defaults; see `CliArgs::apply`.
    let config = args.apply(config::load());
    // Prepare before this process installs its tracing subscriber so terminal mode can reserve
    // stdout for the alternate-screen dashboard. A non-interactive `--tui` falls back to headless
    // operation with ordinary logs, matching the other runtime binaries.
    let terminal_tui = runinator_observability::tui::prepare(args.tui);

    // ensure only one agent runs at a time: two copies would both register the exclusive `desktop`
    // replica and contend for the same pinned/labeled work.
    let _instance = match single_instance::acquire() {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            single_instance::warn_already_running(args.headless || args.tui);
            return ExitCode::SUCCESS;
        }
        // an unexpected bind failure must not lock the operator out of their own agent; note it and
        // start anyway rather than refusing to run.
        Err(err) => {
            eprintln!("desktop-agent single-instance check failed, starting anyway: {err}");
            None
        }
    };

    if args.tui && terminal_tui {
        return match tui::run(&args, config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("desktop agent failed: {err}");
                ExitCode::FAILURE
            }
        };
    }

    if args.headless || args.tui {
        return match headless::run(&args, config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("desktop agent failed: {err}");
                ExitCode::FAILURE
            }
        };
    }

    match service::DesktopAgentService::new().run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("desktop agent failed: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_gui(draft: AgentConfig) -> eframe::Result<()> {
    // share one state handle between the tracing bridge (which writes log lines into it) and the
    // gui (which reads them), starting at the persisted level.
    let shared = Arc::new(Mutex::new(agent::Shared::default()));
    logging::init(shared.clone(), draft.log_level);

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([720.0, 520.0])
            .with_visible(true),
        ..Default::default()
    };

    eframe::run_native(
        "Runinator Desktop Agent",
        native_options,
        Box::new(move |cc| Ok(Box::new(gui::DesktopAgentApp::new(cc, shared, draft)))),
    )
}
