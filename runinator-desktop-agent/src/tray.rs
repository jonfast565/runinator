//! background tray icon so the agent can run headless with the control window tucked away. clicking
//! the icon (or its "Open" menu item) is the only way back to the window; "Exit" is the only way to
//! actually quit — closing the window just hides it, matching the menu-bar-utility convention.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::app_icon;
pub use crate::app_icon::TrayColor;

pub enum TrayAction {
    Open,
    OpenUi,
    Exit,
}

/// owns the tray icon for the process lifetime; dropping it removes the icon from the tray.
pub struct AgentTray {
    tray: TrayIcon,
    open_id: MenuId,
    open_ui_id: MenuId,
    exit_id: MenuId,
}

impl AgentTray {
    /// build the tray icon and its menu. must be called on the main thread after the platform event
    /// loop has started (eframe's app-creator closure is called at the right time for this).
    /// returns `None` if the platform tray failed to initialize; the app still runs, just without a
    /// tray icon, so a failure here should not be fatal.
    pub fn new() -> Option<Self> {
        let open_item = MenuItem::new("Open Runinator Desktop Agent", true, None);
        let open_ui_item = MenuItem::new("Open Command Center", true, None);
        let exit_item = MenuItem::new("Exit", true, None);
        let open_id = open_item.id().clone();
        let open_ui_id = open_ui_item.id().clone();
        let exit_id = exit_item.id().clone();

        let menu = Menu::new();
        menu.append(&open_item).ok()?;
        menu.append(&open_ui_item).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&exit_item).ok()?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Runinator Desktop Agent")
            .with_icon(app_icon::tray_icon(TrayColor::Idle))
            .build()
            .ok()?;

        Some(Self {
            tray,
            open_id,
            open_ui_id,
            exit_id,
        })
    }

    /// Reflect the agent's connection state with a colored-background logo and tooltip. A failing
    /// platform call is ignored rather than propagated.
    pub fn set_status(&self, color: TrayColor, tooltip: &str) {
        let _ = self.tray.set_icon(Some(app_icon::tray_icon(color)));
        let _ = self.tray.set_tooltip(Some(tooltip));
    }

    /// drain one pending tray/menu event, if any. non-blocking; call every frame.
    pub fn poll(&self) -> Option<TrayAction> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.open_id {
                return Some(TrayAction::Open);
            }
            if event.id == self.open_ui_id {
                return Some(TrayAction::OpenUi);
            }
            if event.id == self.exit_id {
                return Some(TrayAction::Exit);
            }
        }

        // a plain left click also opens the window directly, so the menu isn't the only path in.
        if let Ok(TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }) = TrayIconEvent::receiver().try_recv()
        {
            return Some(TrayAction::Open);
        }

        None
    }
}
