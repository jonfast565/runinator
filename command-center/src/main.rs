#![windows_subsystem = "windows"]

use anyhow::Result;
use reqwest::blocking::Client;
use serde::Deserialize;
use slint::{ModelRc, VecModel, Weak, Timer, TimerMode};

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
    model: ModelRc<slint::SharedString>, // dummy to keep type around; unused directly
}

fn main() -> Result<()> {
    let ui = MainWindow::new()?;

    // Backing model for tasks
    let tasks_model: VecModel<TaskItem> = VecModel::default();
    ui.set_tasks(ModelRc::new(tasks_model.clone()));

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

    // Poll every 10s (like your throttle)
    {
        let ui_weak = ui.as_weak();
        let client_clone = client.clone();
        let timer = Timer::default();
        timer.start(
            TimerMode::Repeated,
            std::time::Duration::from_secs(10),
            move || {
                fetch_tasks(ui_weak.clone(), client_clone.clone());
            },
        );
        // Keep _timer alive by moving into UI (optional; Slint keeps timer alive by handle)
        std::mem::forget(timer);
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
                        if let Some(vm) = ui.get_tasks().as_any().downcast_ref::<VecModel<TaskItem>>() {
                            vm.set_vec(items);
                        } else {
                            // first time or replaced: reset model
                            let vm = VecModel::from(items);
                            ui.set_tasks(ModelRc::new(vm));
                        }
                        ui.set_error("".into());
                        // Keep status if any
                    }
                }).ok();
            }
            Err(err) => {
                let msg = err.to_string();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui.upgrade() {
                        ui.set_error(msg.into());
                    }
                }).ok();
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
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui.upgrade() {
                        ui.set_status(status.into());
                        ui.set_error("".into());
                    }
                }).ok();

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
                }).ok();
            }
        }
    });
}
