//! background tray icon so the agent can run headless with the control window tucked away. clicking
//! the icon (or its "Open" menu item) is the only way back to the window; "Exit" is the only way to
//! actually quit — closing the window just hides it, matching the menu-bar-utility convention.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// icon side length in pixels; small tray icons don't benefit from going bigger.
const ICON_SIZE: u32 = 32;

pub enum TrayAction {
    Open,
    OpenUi,
    Exit,
}

/// owns the tray icon for the process lifetime; dropping it removes the icon from the tray.
pub struct AgentTray {
    _tray: TrayIcon,
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
            .with_icon(build_icon())
            .build()
            .ok()?;

        Some(Self {
            _tray: tray,
            open_id,
            open_ui_id,
            exit_id,
        })
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

// a filled circle on a transparent background; enough to be recognizable at tray size without
// shipping an icon asset.
fn build_icon() -> Icon {
    let mut rgba = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let center = ICON_SIZE as f32 / 2.0 - 0.5;
    let radius = ICON_SIZE as f32 / 2.0 - 2.0;

    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let idx = ((y * ICON_SIZE + x) * 4) as usize;
            rgba[idx] = 45;
            rgba[idx + 1] = 140;
            rgba[idx + 2] = 200;
            rgba[idx + 3] = 255;
        }
    }

    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE)
        .expect("tray icon buffer matches ICON_SIZE x ICON_SIZE")
}
