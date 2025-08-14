#![windows_subsystem = "windows"]

use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;
use slint::{Model, ModelRc, Timer, TimerMode, VecModel, Weak};
use std::time::Duration;

slint::include_modules!();

#[derive(Deserialize, Debug, Clone)]
struct TaskResponse {
    success: bool,
    message: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ScheduledTask {
    id: Option<i64>,
    name: String,
    enabled: bool,
}

#[derive(Clone)]
struct AppState {
    client: Client,
    model: ModelRc<slint::SharedString>, // unused placeholder; safe to remove if not needed
}

fn main() -> Result<()> {
    let ui = MainWindow::new()?;

    // Backing model for tasks: move into ModelRc (no clone needed)
    ui.set_tasks(ModelRc::new(VecModel::<TaskItem>::default()));

    // Shared HTTP client
    let client = Client::new();

    // Wire callbacks
    {
        let ui_weak = ui.as_weak();
        let client_clone = client.clone();
        ui.on_refresh(move || {
            fetch_tasks(ui_weak.clone(), client_clone.clone());
        });
    }
    {
        let ui_weak = ui.as_weak();
        let client_clone = client.clone();
        ui.on_run_now(move |id: i32| {
            run_now(ui_weak.clone(), client_clone.clone(), id as i64);
        });
    }

    // Initial load
    {
        let ui_weak = ui.as_weak();
        let client_clone = client.clone();
        fetch_tasks(ui_weak, client_clone);
    }

    // Poll every 10s
    let refresh_timer = Timer::default();
    {
        let ui_weak = ui.as_weak();
        let client_clone = client.clone();
        refresh_timer.start(
            TimerMode::Repeated,
            Duration::from_secs(10),
            move || {
                fetch_tasks(ui_weak.clone(), client_clone.clone());
            },
        );
    }

    ui.run()?;
    Ok(())
}

fn fetch_tasks(ui: Weak<MainWindow>, client: Client) {
    std::thread::spawn(move || {
        let res = client
            .get("http://localhost:3001/tasks")
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json::<Vec<ScheduledTask>>());

        match res {
            Ok(list) => {
                let items: Vec<TaskItem> = list
                    .into_iter()
                    .map(|t| TaskItem {
                        id: t.id.unwrap_or_default() as i32,
                        name: t.name.into(),
                        enabled: t.enabled,
                    })
                    .collect();

                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui.upgrade() {
                        if let Some(vm) =
                            ui.get_tasks().as_any().downcast_ref::<VecModel<TaskItem>>()
                        {
                            vm.set_vec(items);
                        } else {
                            let vm = VecModel::from(items);
                            ui.set_tasks(ModelRc::new(vm));
                        }
                        ui.set_error("".into());
                    }
                })
                .ok();
            }
            Err(err) => {
                let msg = err.to_string();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui.upgrade() {
                        ui.set_error(msg.into());
                    }
                })
                .ok();
            }
        }
    });
}

fn run_now(ui: Weak<MainWindow>, client: Client, id: i64) {
    std::thread::spawn(move || {
        let res = client
            .post(&format!("http://localhost:3001/tasks/{}/request_run", id))
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json::<TaskResponse>());

        match res {
            Ok(resp) => {
                let status = format!(
                    "{}: {}",
                    if resp.success { "OK" } else { "ERR" },
                    resp.message
                );

                // Clone Weak before moving into the event-loop closure
                let ui_for_status = ui.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_for_status.upgrade() {
                        ui.set_status(status.into());
                        ui.set_error("".into());
                    }
                })
                .ok();

                // Auto-refresh after run
                let ui2 = ui.clone();
                let client2 = client.clone();
                fetch_tasks(ui2, client2);
            }
            Err(err) => {
                let msg = err.to_string();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui.upgrade() {
                        ui.set_error(msg.into());
                    }
                })
                .ok();
            }
        }
    });
}
