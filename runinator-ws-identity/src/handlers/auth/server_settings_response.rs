#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct ServerSettingsResponse {
    pub(super) values: ServerSettings,
    pub(super) catalog: Vec<runinator_models::server_settings::ServerSettingDefinition>,
    pub(super) runtime_catalog: Vec<RuntimeSettingDefinition>,
}
