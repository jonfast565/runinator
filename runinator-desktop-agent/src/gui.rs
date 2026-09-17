//! the desktop agent's control surface: a small window to configure the sandbox folder, start the
//! worker loop — or cancel a start still coming up — stop it, and watch its status. Its live
//! dashboard mirrors the terminal host's operational view without turning this into a workflow
//! editor. The window's close button asks whether to hide it behind the tray icon or quit.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::agent::{
    self, AgentConfig, AgentMetrics, AgentStatus, ConnectionState, Control, SharedHandle,
};
use crate::config::{self, LogLevel, WindowCloseAction};
use crate::logging;
use crate::tray::{AgentTray, TrayAction, TrayColor};
use runinator_platform::time::format_duration;
use runinator_worker::ActionOutcome;

// presets offered by the optional-label type-ahead. `pool=desktop` and `runner=desktop` are shown
// separately as fixed identity labels, so suggestions only cover user-configurable routing facts.
const LABEL_SUGGESTIONS: &[&str] = &["zone=home", "capability=desktop"];
const REQUIRED_LABELS: &[&str] = &["pool=desktop", "runner=desktop"];

fn display_duration(duration: Duration) -> String {
    format_duration(duration)
}

/// a per-frame copy of the shared agent state the GUI renders from, taken under one short lock.

/// locally persisted approval can be fresher than the background collection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileApprovalState {
    Approved,
    Required,
    Changed,
    Disabled,
}

fn profile_approval_state(
    enabled: bool,
    config_digest: &str,
    approved_digest: Option<&str>,
) -> ProfileApprovalState {
    if !enabled {
        ProfileApprovalState::Disabled
    } else {
        match approved_digest {
            Some(digest) if digest == config_digest => ProfileApprovalState::Approved,
            Some(_) => ProfileApprovalState::Changed,
            None => ProfileApprovalState::Required,
        }
    }
}

fn profile_approval_presentation(state: ProfileApprovalState) -> (&'static str, egui::Color32) {
    match state {
        ProfileApprovalState::Approved => ("Approved on this computer", DOT_GREEN),
        ProfileApprovalState::Required => ("Not approved on this computer", DOT_AMBER),
        ProfileApprovalState::Changed => ("Approval needs renewal", DOT_AMBER),
        ProfileApprovalState::Disabled => ("Disabled centrally", DOT_GRAY),
    }
}

// the status-dot palette. amber and red are the two the operator is meant to tell apart at a
// glance: amber is "still trying", red is "stopped, and not coming back without you".
const DOT_GRAY: egui::Color32 = egui::Color32::from_rgb(130, 130, 130);
const DOT_BLUE: egui::Color32 = egui::Color32::from_rgb(45, 140, 200);
const DOT_GREEN: egui::Color32 = egui::Color32::from_rgb(64, 180, 96);
const DOT_AMBER: egui::Color32 = egui::Color32::from_rgb(220, 170, 45);
const DOT_RED: egui::Color32 = egui::Color32::from_rgb(210, 70, 70);

/// How a connection state renders: header dot text + color, and the matching tray logo/tooltip.

fn present_status(connection: &ConnectionState, busy: bool) -> StatusPresentation {
    // a start/stop transition in flight reads as "working" regardless of the underlying phase.
    if busy {
        return StatusPresentation {
            label: "working…".to_string(),
            color: DOT_AMBER,
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — working…".to_string(),
        };
    }
    match connection {
        ConnectionState::Stopped => StatusPresentation {
            label: "stopped".to_string(),
            color: DOT_GRAY,
            tray_color: TrayColor::Idle,
            tooltip: "Runinator Desktop Agent — stopped".to_string(),
        },
        ConnectionState::Registering => StatusPresentation {
            label: "registering…".to_string(),
            color: DOT_BLUE,
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — registering…".to_string(),
        },
        ConnectionState::Connecting => StatusPresentation {
            label: "connecting…".to_string(),
            color: DOT_BLUE,
            tray_color: TrayColor::Connecting,
            tooltip: "Runinator Desktop Agent — connecting…".to_string(),
        },
        ConnectionState::Connected => StatusPresentation {
            label: "running".to_string(),
            color: DOT_GREEN,
            tray_color: TrayColor::Connected,
            tooltip: "Runinator Desktop Agent — running".to_string(),
        },
        ConnectionState::Reconnecting {
            retry_secs,
            attempt,
            max_attempts,
        } => StatusPresentation {
            label: format!("reconnecting{}", attempt_suffix(*attempt, *max_attempts)),
            color: DOT_AMBER,
            tray_color: TrayColor::Reconnecting,
            tooltip: format!(
                "Runinator Desktop Agent — reconnecting{} (retry in {retry_secs}s)",
                attempt_suffix(*attempt, *max_attempts)
            ),
        },
        ConnectionState::Disconnected { attempts, reason } => StatusPresentation {
            label: "disconnected".to_string(),
            color: DOT_RED,
            tray_color: TrayColor::Disconnected,
            tooltip: format!(
                "Runinator Desktop Agent — disconnected after {attempts} attempts ({reason})"
            ),
        },
        ConnectionState::ReenrollmentRequired { reason } => StatusPresentation {
            label: "re-enrollment required".to_string(),
            color: DOT_RED,
            tray_color: TrayColor::Disconnected,
            tooltip: format!("Runinator Desktop Agent — credential rejected ({reason})"),
        },
    }
}

// " 3/10", or nothing when the agent retries indefinitely — the count only means something against
// a budget.
fn attempt_suffix(attempt: u32, max_attempts: Option<u32>) -> String {
    match max_attempts {
        Some(max) => format!(" {attempt}/{max}"),
        None => String::new(),
    }
}

/// the first reason the current draft can't start, or `None` when it's good to go. drives the Start
/// button's enabled state and its disabled-hover explanation, so a misconfiguration is caught here
/// rather than after the worker loop has already spun up.
fn validate_config(draft: &AgentConfig) -> Option<String> {
    let service_url = draft.service_url.trim();
    if service_url.is_empty() {
        return Some("Set a service URL.".to_string());
    }
    match reqwest::Url::parse(service_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => {}
        Ok(_) => return Some("Service URL must be http:// or https://.".to_string()),
        Err(_) => return Some("Service URL is not a valid URL.".to_string()),
    }

    let root = draft.sandbox_root.trim();
    if root.is_empty() {
        return Some("Choose a sandbox folder.".to_string());
    }
    if !std::path::Path::new(root).is_dir() {
        return Some("Sandbox folder does not exist.".to_string());
    }

    let working_dir = draft.console_working_dir.trim();
    if !working_dir.is_empty() && !std::path::Path::new(working_dir).is_dir() {
        return Some("Working directory does not exist.".to_string());
    }

    if draft.broker_mode == config::BrokerMode::Direct {
        if draft.direct_broker_backend.trim().is_empty() {
            return Some("Set a broker backend for Direct mode.".to_string());
        }
        if draft.direct_broker_endpoint.trim().is_empty() {
            return Some("Set a broker endpoint for Direct mode.".to_string());
        }
    }

    None
}

fn compact_detail(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(format!("{label}: {value}"))
            .small()
            .weak(),
    );
}

fn outcome_presentation(outcome: ActionOutcome) -> (&'static str, egui::Color32) {
    match outcome {
        ActionOutcome::Succeeded => ("Succeeded", DOT_GREEN),
        ActionOutcome::Failed => ("Failed", DOT_RED),
        ActionOutcome::TimedOut => ("Timed out", DOT_AMBER),
        ActionOutcome::Canceled => ("Canceled", DOT_GRAY),
    }
}

fn outcome_total(ui: &mut egui::Ui, outcome: ActionOutcome, total: u64) {
    let (label, color) = outcome_presentation(outcome);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        outcome_mark(ui, outcome).on_hover_text(label);
        ui.colored_label(color, format!("{label}: {total}"));
    });
}

/// Paint activity marks ourselves: the default egui fonts do not cover the miscellaneous Unicode
/// check, ballot, and hourglass glyphs consistently, which otherwise turns these metrics into tofu
/// boxes on some platforms.
fn outcome_mark(ui: &mut egui::Ui, outcome: ActionOutcome) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    let (_, color) = outcome_presentation(outcome);
    let stroke = egui::Stroke::new(1.7_f32, color);
    let center = rect.center();

    match outcome {
        ActionOutcome::Succeeded => {
            ui.painter().line_segment(
                [
                    center + egui::vec2(-5.0, 0.0),
                    center + egui::vec2(-1.5, 3.5),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    center + egui::vec2(-1.5, 3.5),
                    center + egui::vec2(5.0, -4.0),
                ],
                stroke,
            );
        }
        ActionOutcome::Failed => {
            ui.painter().line_segment(
                [
                    center + egui::vec2(-4.0, -4.0),
                    center + egui::vec2(4.0, 4.0),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    center + egui::vec2(4.0, -4.0),
                    center + egui::vec2(-4.0, 4.0),
                ],
                stroke,
            );
        }
        ActionOutcome::TimedOut => {
            ui.painter().circle_stroke(center, 5.0, stroke);
            ui.painter()
                .line_segment([center, center + egui::vec2(0.0, -3.0)], stroke);
            ui.painter()
                .line_segment([center, center + egui::vec2(2.5, 1.5)], stroke);
        }
        ActionOutcome::Canceled => {
            ui.painter().circle_stroke(center, 5.0, stroke);
            ui.painter().line_segment(
                [
                    center + egui::vec2(-3.5, 3.5),
                    center + egui::vec2(3.5, -3.5),
                ],
                stroke,
            );
        }
    }

    response
}

fn resource_chart(
    ui: &mut egui::Ui,
    title: &str,
    value: &str,
    values: &[f64],
    fixed_max: Option<f64>,
    color: egui::Color32,
) {
    ui.group(|ui| {
        // Keep the label and its value on independent lines. Long values (notably total RAM and
        // broker-derived units) can then wrap naturally instead of colliding in a narrow card.
        ui.label(egui::RichText::new(title).small().strong());
        ui.label(egui::RichText::new(value).small().weak());
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
        let values = values
            .iter()
            .copied()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .collect::<Vec<_>>();
        if values.len() < 2 || rect.width() <= 1.0 {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "collecting…",
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
            return;
        }
        let max = fixed_max
            .unwrap_or_else(|| values.iter().copied().fold(0.0_f64, f64::max))
            .max(1.0);
        let points = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let x = rect.left() + rect.width() * index as f32 / (values.len() - 1) as f32;
                let y = rect.bottom() - rect.height() * (*value / max).clamp(0.0, 1.0) as f32;
                egui::pos2(x, y)
            })
            .collect::<Vec<_>>();
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0_f32, DOT_GRAY),
        );
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
    });
}

/// A painted light, rather than a font-dependent Unicode glyph, makes the current lifecycle state
/// readable at a glance on every platform and at every UI scale.
fn status_light(ui: &mut egui::Ui, presentation: &StatusPresentation) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let center = rect.center();
    ui.painter().circle_filled(center, 5.5, presentation.color);
    ui.painter().circle_stroke(
        center,
        5.5,
        egui::Stroke::new(1.0_f32, ui.visuals().widgets.inactive.bg_fill),
    );
    response.on_hover_text(&presentation.tooltip);
}

fn format_bytes(value: f64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = value.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
#[path = "gui_tests.rs"]
mod tests;

mod snapshot;
use snapshot::Snapshot;

mod profile_approval_feedback;
use profile_approval_feedback::ProfileApprovalFeedback;

mod runtime_dashboard;
use runtime_dashboard::RuntimeDashboard;

mod status_presentation;
use status_presentation::StatusPresentation;

mod desktop_agent_app;
pub use desktop_agent_app::DesktopAgentApp;
