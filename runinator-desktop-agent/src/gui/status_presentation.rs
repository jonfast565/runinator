#[allow(unused_imports)]
use super::*;

pub(super) struct StatusPresentation {
    pub(super) label: String,
    pub(super) color: egui::Color32,
    pub(super) tray_color: TrayColor,
    pub(super) tooltip: String,
}
