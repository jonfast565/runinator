#[allow(unused_imports)]
use super::*;

pub(super) struct ProfileApprovalFeedback {
    pub(super) profile_id: uuid::Uuid,
    pub(super) message: String,
    pub(super) color: egui::Color32,
}
