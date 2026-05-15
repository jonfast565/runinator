use tauri::Manager;

use crate::{discovery::start_discovery_thread, state::CommandCenterState};

pub fn run() {
    tauri::Builder::default()
        .manage(CommandCenterState::new())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<CommandCenterState>().inner().clone();
            start_discovery_thread(handle, state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::get_service_status,
            crate::commands::start_service_discovery,
            crate::commands::save_workflow_bundle,
            crate::commands::delete_workflow,
            crate::commands::fetch_run_chunks,
            crate::commands::fetch_run_artifacts,
            crate::commands::fetch_workflow_node_run_chunks,
            crate::commands::fetch_workflow_node_run_artifacts,
            crate::commands::fetch_workflows,
            crate::commands::save_workflow,
            crate::commands::fetch_workflow_triggers,
            crate::commands::save_workflow_trigger,
            crate::commands::delete_workflow_trigger,
            crate::commands::create_workflow_run,
            crate::commands::step_workflow_run,
            crate::commands::fetch_workflow_runs,
            crate::commands::fetch_workflow_run,
            crate::commands::fetch_resource_records,
            crate::commands::fetch_providers,
            crate::commands::fetch_credentials,
            crate::commands::save_credential,
            crate::commands::delete_credential,
            crate::commands::approve_approval,
            crate::commands::reject_approval
        ])
        .run(tauri::generate_context!())
        .expect("failed to run command center");
}
