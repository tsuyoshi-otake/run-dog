use std::{mem::size_of, ptr};

use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
            NIM_SETVERSION, NIN_POPUPCLOSE, NIN_POPUPOPEN, NIN_SELECT, NOTIFYICONDATAW,
            NOTIFYICON_VERSION_4,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, KillTimer, PostMessageW,
            SetForegroundWindow, SetTimer, TrackPopupMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP,
            MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_RIGHTBUTTON, WM_CONTEXTMENU,
            WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP,
        },
    },
};

use crate::{
    application::{Effect, Event, TimerKind, TrayIcon},
    core::{FpsLimit, ThemePreference},
};

use super::{flyout::HoverFlyout, icons::IconFrames, update::UpdateMenuState};

pub const TRAY_CALLBACK_MESSAGE: u32 = 0x8000 + 1;
/// One-shot retries while Explorer populates `NotifyIconSettings`.
pub const PROMOTE_TIMER_ID: usize = 3;
const PROMOTE_RETRY_MS: u32 = 500;
const PROMOTE_MAX_ATTEMPTS: u8 = 10;

pub const COMMAND_THEME_SYSTEM: u32 = 1_001;
pub const COMMAND_THEME_LIGHT: u32 = 1_002;
pub const COMMAND_THEME_DARK: u32 = 1_003;
pub const COMMAND_FPS_10: u32 = 1_010;
pub const COMMAND_FPS_20: u32 = 1_011;
pub const COMMAND_FPS_30: u32 = 1_012;
pub const COMMAND_FPS_40: u32 = 1_013;
pub const COMMAND_TOGGLE_STARTUP: u32 = 1_020;
pub const COMMAND_CHECK_FOR_UPDATES: u32 = 1_030;
pub const COMMAND_INSTALL_UPDATE: u32 = 1_031;
pub const COMMAND_EXIT: u32 = 1_099;

/// Converts a menu command to a pure application event.
#[must_use]
pub const fn event_for_command(command: u32) -> Option<Event> {
    match command {
        COMMAND_THEME_SYSTEM => Some(Event::SelectTheme(ThemePreference::System)),
        COMMAND_THEME_LIGHT => Some(Event::SelectTheme(ThemePreference::Light)),
        COMMAND_THEME_DARK => Some(Event::SelectTheme(ThemePreference::Dark)),
        COMMAND_FPS_10 => Some(Event::SelectFpsLimit(FpsLimit::Fps10)),
        COMMAND_FPS_20 => Some(Event::SelectFpsLimit(FpsLimit::Fps20)),
        COMMAND_FPS_30 => Some(Event::SelectFpsLimit(FpsLimit::Fps30)),
        COMMAND_FPS_40 => Some(Event::SelectFpsLimit(FpsLimit::Fps40)),
        COMMAND_TOGGLE_STARTUP => Some(Event::ToggleStartup),
        COMMAND_EXIT => Some(Event::ExitRequested),
        _ => None,
    }
}

/// Shell tray and small context-menu adapter. It owns long-lived HICON handles
/// and otherwise retains only scalar menu state.
pub struct TrayAdapter {
    hwnd: HWND,
    icons: IconFrames,
    theme: ThemePreference,
    fps_limit: FpsLimit,
    startup_enabled: bool,
    added: bool,
    promote_attempts: u8,
    flyout: HoverFlyout,
    last_icon: Option<TrayIcon>,
}

impl TrayAdapter {
    #[must_use]
    pub fn new(
        icons: IconFrames,
        theme: ThemePreference,
        fps_limit: FpsLimit,
        startup: bool,
    ) -> Self {
        Self {
            hwnd: ptr::null_mut(),
            icons,
            theme,
            fps_limit,
            startup_enabled: startup,
            added: false,
            promote_attempts: 0,
            flyout: HoverFlyout::new(),
            last_icon: None,
        }
    }

    pub fn set_window(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
    }

    pub fn apply(&mut self, effect: &Effect) {
        match effect {
            Effect::AddTray(icon) => {
                self.add(icon);
                self.begin_promote();
            }
            Effect::ModifyTray(icon) => self.modify(icon),
            Effect::RemoveTray => {
                self.stop_promote();
                self.flyout.destroy();
                self.last_icon = None;
                self.remove();
            }
            Effect::SetThemeMenu(theme) => self.theme = *theme,
            Effect::SetFpsMenu(limit) => self.fps_limit = *limit,
            Effect::SetStartupMenu(enabled) => self.startup_enabled = *enabled,
            Effect::SetTimer { .. }
            | Effect::KillTimer(TimerKind::CpuSampling | TimerKind::Animation)
            | Effect::SaveSettings(_)
            | Effect::CommitSettings { .. }
            | Effect::CancelCommit { .. }
            | Effect::LaunchTaskManager
            | Effect::Quit => {}
        }
    }

    /// Opens the right-click menu. Menu handles exist only for this invocation.
    pub fn show_menu(&mut self, update_state: &UpdateMenuState) {
        self.flyout.hide();
        let root = unsafe { CreatePopupMenu() };
        let theme_menu = unsafe { CreatePopupMenu() };
        let speed_menu = unsafe { CreatePopupMenu() };
        if root.is_null() || theme_menu.is_null() || speed_menu.is_null() {
            if !root.is_null() {
                let _ = unsafe { DestroyMenu(root) };
            }
            if !theme_menu.is_null() {
                let _ = unsafe { DestroyMenu(theme_menu) };
            }
            if !speed_menu.is_null() {
                let _ = unsafe { DestroyMenu(speed_menu) };
            }
            return;
        }

        append_checked(
            theme_menu,
            COMMAND_THEME_SYSTEM,
            "System",
            self.theme == ThemePreference::System,
        );
        append_checked(
            theme_menu,
            COMMAND_THEME_LIGHT,
            "Light",
            self.theme == ThemePreference::Light,
        );
        append_checked(
            theme_menu,
            COMMAND_THEME_DARK,
            "Dark",
            self.theme == ThemePreference::Dark,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_10,
            "10 FPS",
            self.fps_limit == FpsLimit::Fps10,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_20,
            "20 FPS",
            self.fps_limit == FpsLimit::Fps20,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_30,
            "30 FPS",
            self.fps_limit == FpsLimit::Fps30,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_40,
            "40 FPS",
            self.fps_limit == FpsLimit::Fps40,
        );

        append_submenu(root, theme_menu, "Theme");
        append_submenu(root, speed_menu, "Maximum animation speed");
        let _ = unsafe { AppendMenuW(root, MF_SEPARATOR, 0, ptr::null()) };
        append_checked(
            root,
            COMMAND_TOGGLE_STARTUP,
            "Launch at startup",
            self.startup_enabled,
        );
        let _ = unsafe { AppendMenuW(root, MF_SEPARATOR, 0, ptr::null()) };
        append_update_menu(root, update_state);
        let _ = unsafe { AppendMenuW(root, MF_SEPARATOR, 0, ptr::null()) };
        append_checked(root, COMMAND_EXIT, "Exit", false);

        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) } != 0 {
            let _ = unsafe { SetForegroundWindow(self.hwnd) };
            let _ = unsafe {
                TrackPopupMenu(
                    root,
                    TPM_RIGHTBUTTON,
                    point.x,
                    point.y,
                    0,
                    self.hwnd,
                    ptr::null(),
                )
            };
            // Shell tray menus need a follow-up message after `TrackPopupMenu`.
            // Without it Windows can immediately dismiss the popup or keep focus
            // on the notification area instead of the menu owner.
            let _ = unsafe { PostMessageW(self.hwnd, WM_NULL, 0, 0) };
        }
        // Destroying the root also destroys its attached submenus.
        let _ = unsafe { DestroyMenu(root) };
    }

    #[must_use]
    pub const fn is_context_menu_notification(notification: u32) -> bool {
        notification == WM_RBUTTONUP || notification == WM_CONTEXTMENU
    }

    /// Extracts the Shell notification code from the callback `lParam`.
    ///
    /// `NOTIFYICON_VERSION_4` packs the event in the low word and the icon ID
    /// in the high word. Earlier versions place only the event in `lParam`, so
    /// masking is correct for both wire formats.
    #[must_use]
    pub const fn notification_code(callback_lparam: u32) -> u32 {
        callback_lparam & 0xFFFF
    }

    #[must_use]
    pub const fn is_activation_notification(notification: u32) -> bool {
        notification == WM_LBUTTONDBLCLK || notification == NIN_SELECT || notification == 1_025
    }

    #[must_use]
    pub const fn is_popup_open_notification(notification: u32) -> bool {
        notification == NIN_POPUPOPEN
    }

    #[must_use]
    pub const fn is_popup_close_notification(notification: u32) -> bool {
        notification == NIN_POPUPCLOSE
    }

    pub fn handle_hover(&mut self, notification: u32) {
        if Self::is_popup_close_notification(notification) {
            self.flyout.hide();
            return;
        }
        if Self::is_popup_open_notification(notification) && self.last_icon.is_some() {
            self.flyout.show_near_icon(self.hwnd);
        }
    }

    /// Explorer may create the per-icon settings subkey after `NIM_ADD`.
    pub fn on_promote_timer(&mut self) {
        self.promote_once();
    }

    fn begin_promote(&mut self) {
        self.promote_attempts = PROMOTE_MAX_ATTEMPTS;
        self.promote_once();
    }

    fn promote_once(&mut self) {
        if self.promote_attempts == 0 || !self.added {
            self.stop_promote();
            return;
        }
        self.promote_attempts -= 1;
        match super::notify_icon::try_promote_current_executable() {
            super::notify_icon::PromoteAttempt::Done => self.stop_promote(),
            super::notify_icon::PromoteAttempt::Retry if self.promote_attempts > 0 => {
                self.arm_promote_timer();
            }
            super::notify_icon::PromoteAttempt::Retry => self.stop_promote(),
        }
    }

    fn arm_promote_timer(&mut self) {
        if self.hwnd.is_null() {
            self.stop_promote();
            return;
        }
        let _ = unsafe { SetTimer(self.hwnd, PROMOTE_TIMER_ID, PROMOTE_RETRY_MS, None) };
    }

    fn stop_promote(&mut self) {
        self.promote_attempts = 0;
        if !self.hwnd.is_null() {
            let _ = unsafe { KillTimer(self.hwnd, PROMOTE_TIMER_ID) };
        }
    }

    fn add(&mut self, icon: &TrayIcon) {
        self.remember(icon);
        let data = self.notification_data(icon);
        if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
            self.added = false;
            return;
        }
        let mut version_data = data;
        version_data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &version_data) };
        self.added = true;
    }

    fn modify(&mut self, icon: &TrayIcon) {
        self.remember(icon);
        if !self.added {
            self.add(icon);
            return;
        }
        let data = self.notification_data(icon);
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } == 0 {
            self.add(icon);
        }
    }

    fn remove(&mut self) {
        if !self.added {
            return;
        }
        let data = self.notification_data(&TrayIcon {
            theme: crate::core::ResolvedTheme::Dark,
            frame: 0,
            tooltip: String::new(),
            cpu_sparkline: crate::core::Sparkline::new(),
            memory_sparkline: crate::core::Sparkline::new(),
            cpu_breakdown: None,
            memory: None,
            storage: None,
            usage: crate::core::UsageSnapshot::default(),
        });
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
        self.added = false;
    }

    fn notification_data(&self, icon: &TrayIcon) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MESSAGE,
            hIcon: self.icons.icon(icon.theme, icon.frame),
            ..NOTIFYICONDATAW::default()
        };
        let limit = data.szTip.len() - 1;
        for (slot, utf16) in data
            .szTip
            .iter_mut()
            .take(limit)
            .zip(icon.tooltip.encode_utf16())
        {
            *slot = utf16;
        }
        data
    }

    fn remember(&mut self, icon: &TrayIcon) {
        let display_changed = self.last_icon.as_ref().is_none_or(|previous| {
            previous.theme != icon.theme
                || previous.tooltip != icon.tooltip
                || previous.cpu_sparkline != icon.cpu_sparkline
                || previous.memory_sparkline != icon.memory_sparkline
                || previous.cpu_breakdown != icon.cpu_breakdown
                || previous.memory != icon.memory
                || previous.storage != icon.storage
                || previous.usage != icon.usage
        });
        self.last_icon = Some(icon.clone());
        if display_changed {
            self.flyout.set_state(icon);
        }
    }
}

fn append_submenu(parent: HMENU, submenu: HMENU, label: &str) {
    let label = wide(label);
    let _ = unsafe {
        AppendMenuW(
            parent,
            MF_POPUP | MF_STRING,
            submenu as usize,
            label.as_ptr(),
        )
    };
}

fn append_checked(menu: HMENU, command: u32, label: &str, checked: bool) {
    let label = wide(label);
    let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
    let _ = unsafe { AppendMenuW(menu, flags, command as usize, label.as_ptr()) };
}

fn append_update_menu(menu: HMENU, state: &UpdateMenuState) {
    match state {
        UpdateMenuState::Idle => {
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, "Check for updates");
        }
        UpdateMenuState::Checking => {
            append_disabled(menu, "Checking for updates...");
        }
        UpdateMenuState::Current => {
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, "Check for updates");
            append_disabled(menu, "RunDog is up to date");
        }
        UpdateMenuState::Available { version } => {
            append_action(
                menu,
                COMMAND_INSTALL_UPDATE,
                &format!("Install RunDog v{version}"),
            );
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, "Check again");
        }
        UpdateMenuState::Downloading { version } => {
            append_disabled(menu, &format!("Downloading RunDog v{version}..."));
        }
        UpdateMenuState::Launching => {
            append_disabled(menu, "Starting installer...");
        }
        UpdateMenuState::Failed => {
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, "Retry update check");
            append_disabled(menu, "Update could not be completed");
        }
    }
}

fn append_action(menu: HMENU, command: u32, label: &str) {
    let label = wide(label);
    let _ = unsafe { AppendMenuW(menu, MF_STRING, command as usize, label.as_ptr()) };
}

fn append_disabled(menu: HMENU, label: &str) {
    let label = wide(label);
    let _ = unsafe { AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, label.as_ptr()) };
}

#[must_use]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        event_for_command, TrayAdapter, COMMAND_CHECK_FOR_UPDATES, COMMAND_EXIT, COMMAND_FPS_40,
        COMMAND_THEME_DARK, COMMAND_TOGGLE_STARTUP,
    };
    use crate::{
        application::Event,
        core::{FpsLimit, ThemePreference},
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_CONTEXTMENU, WM_RBUTTONUP};

    #[test]
    fn component_command_mapping_covers_known_and_unknown_input_partitions() {
        assert_eq!(
            event_for_command(COMMAND_THEME_DARK),
            Some(Event::SelectTheme(ThemePreference::Dark))
        );
        assert_eq!(
            event_for_command(COMMAND_FPS_40),
            Some(Event::SelectFpsLimit(FpsLimit::Fps40))
        );
        assert_eq!(
            event_for_command(COMMAND_TOGGLE_STARTUP),
            Some(Event::ToggleStartup)
        );
        assert_eq!(event_for_command(COMMAND_EXIT), Some(Event::ExitRequested));
        assert_eq!(event_for_command(COMMAND_CHECK_FOR_UPDATES), None);
        assert_eq!(event_for_command(0), None);
    }

    #[test]
    fn component_tray_notification_classifier_covers_click_kinds() {
        let v4_right_click = TrayAdapter::notification_code((1 << 16) | WM_RBUTTONUP);
        let v4_context_menu = TrayAdapter::notification_code((1 << 16) | WM_CONTEXTMENU);

        assert!(TrayAdapter::is_context_menu_notification(v4_right_click));
        assert!(TrayAdapter::is_context_menu_notification(v4_context_menu));
        assert!(!TrayAdapter::is_context_menu_notification(515));
        assert!(TrayAdapter::is_activation_notification(515));
        assert!(TrayAdapter::is_activation_notification(1024));
        assert!(TrayAdapter::is_activation_notification(1025));
        assert!(!TrayAdapter::is_activation_notification(0));
        assert!(TrayAdapter::is_popup_open_notification(0x0406));
        assert!(TrayAdapter::is_popup_close_notification(0x0407));
        assert!(!TrayAdapter::is_popup_open_notification(0x0400));
    }
}
