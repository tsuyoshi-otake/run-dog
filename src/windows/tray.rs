use std::{mem::size_of, ptr};

use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND,
            NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_POPUPCLOSE, NIN_POPUPOPEN,
            NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, KillTimer, PostMessageW,
            SetForegroundWindow, SetTimer, TrackPopupMenu, HICON, HMENU, MF_CHECKED, MF_GRAYED,
            MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_RIGHTBUTTON, WM_CONTEXTMENU,
            WM_NULL,
        },
    },
};

use crate::{
    application::{Effect, Event, TimerKind, TrayIcon},
    core::{format_tray_glyph, FpsLimit, ThemePreference, TrayDisplayMode},
};

use super::{
    flyout::HoverFlyout,
    icons::{GeneratedIcon, IconFrames},
    update::UpdateMenuState,
};

pub use super::messages::TRAY_CALLBACK_MESSAGE;

// windows-sys 0.61 does not expose this Shell notification constant.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
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
pub const COMMAND_DISPLAY_DOG: u32 = 1_050;
pub const COMMAND_DISPLAY_CPU: u32 = 1_051;
pub const COMMAND_DISPLAY_MEMORY: u32 = 1_052;
pub const COMMAND_DISPLAY_GPU: u32 = 1_053;
pub const COMMAND_DISPLAY_CLAUDE_5H: u32 = 1_054;
pub const COMMAND_DISPLAY_CLAUDE_WEEK: u32 = 1_055;
pub const COMMAND_DISPLAY_FABLE_WEEK: u32 = 1_056;
pub const COMMAND_DISPLAY_CODEX_5H: u32 = 1_057;
pub const COMMAND_DISPLAY_CODEX_WEEK: u32 = 1_058;
pub const COMMAND_TOGGLE_STARTUP: u32 = 1_020;
pub const COMMAND_TOGGLE_PINNED_FLYOUT: u32 = 1_021;
pub const COMMAND_TOGGLE_AUTO_UPDATE: u32 = 1_022;
pub const COMMAND_CHECK_FOR_UPDATES: u32 = 1_030;
pub const COMMAND_INSTALL_UPDATE: u32 = 1_031;
pub const COMMAND_RESCAN_MONTH_USAGE: u32 = 1_032;
pub const COMMAND_ABOUT: u32 = 1_040;
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
        COMMAND_DISPLAY_DOG => Some(Event::SelectDisplayMode(TrayDisplayMode::Dog)),
        COMMAND_DISPLAY_CPU => Some(Event::SelectDisplayMode(TrayDisplayMode::Cpu)),
        COMMAND_DISPLAY_MEMORY => Some(Event::SelectDisplayMode(TrayDisplayMode::Memory)),
        COMMAND_DISPLAY_GPU => Some(Event::SelectDisplayMode(TrayDisplayMode::Gpu)),
        COMMAND_DISPLAY_CLAUDE_5H => Some(Event::SelectDisplayMode(TrayDisplayMode::Claude5h)),
        COMMAND_DISPLAY_CLAUDE_WEEK => Some(Event::SelectDisplayMode(TrayDisplayMode::ClaudeWeek)),
        COMMAND_DISPLAY_FABLE_WEEK => Some(Event::SelectDisplayMode(TrayDisplayMode::FableWeek)),
        COMMAND_DISPLAY_CODEX_5H => Some(Event::SelectDisplayMode(TrayDisplayMode::Codex5h)),
        COMMAND_DISPLAY_CODEX_WEEK => Some(Event::SelectDisplayMode(TrayDisplayMode::CodexWeek)),
        COMMAND_TOGGLE_STARTUP => Some(Event::ToggleStartup),
        COMMAND_TOGGLE_AUTO_UPDATE => Some(Event::ToggleAutoUpdate),
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
    display_mode: TrayDisplayMode,
    startup_enabled: bool,
    auto_update_enabled: bool,
    flyout_pinned: bool,
    added: bool,
    promote_attempts: u8,
    flyout: HoverFlyout,
    last_icon: Option<TrayIcon>,
    text_icon: Option<GeneratedIcon>,
    text_key: Option<(crate::core::ResolvedTheme, String)>,
}

impl TrayAdapter {
    #[must_use]
    pub fn new(
        icons: IconFrames,
        theme: ThemePreference,
        fps_limit: FpsLimit,
        startup: bool,
        auto_update: bool,
        display_mode: TrayDisplayMode,
    ) -> Self {
        Self {
            hwnd: ptr::null_mut(),
            icons,
            theme,
            fps_limit,
            display_mode,
            startup_enabled: startup,
            auto_update_enabled: auto_update,
            flyout_pinned: super::registry::load_pinned_flyout(),
            added: false,
            promote_attempts: 0,
            flyout: HoverFlyout::new(),
            last_icon: None,
            text_icon: None,
            text_key: None,
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
                if self.flyout_pinned {
                    self.show_pinned_flyout();
                }
            }
            Effect::ModifyTray(icon) => self.modify(icon),
            Effect::RemoveTray => {
                self.stop_promote();
                self.flyout.destroy();
                self.last_icon = None;
                self.text_icon = None;
                self.text_key = None;
                self.remove();
            }
            Effect::SetThemeMenu(theme) => self.theme = *theme,
            Effect::SetFpsMenu(limit) => self.fps_limit = *limit,
            Effect::SetDisplayMenu(mode) => self.display_mode = *mode,
            Effect::SetStartupMenu(enabled) => self.startup_enabled = *enabled,
            Effect::NotifyStartupChanged(enabled) => {
                let text = super::i18n::current().menu();
                self.show_balloon(
                    "RunDog",
                    if *enabled {
                        text.balloon_startup_on
                    } else {
                        text.balloon_startup_off
                    },
                );
            }
            Effect::SetAutoUpdateMenu(enabled) => self.auto_update_enabled = *enabled,
            Effect::NotifyAutoUpdateChanged(enabled) => {
                let text = super::i18n::current().menu();
                self.show_balloon(
                    "RunDog",
                    if *enabled {
                        text.balloon_auto_update_on
                    } else {
                        text.balloon_auto_update_off
                    },
                );
            }
            Effect::SetTimer { .. }
            | Effect::KillTimer(TimerKind::CpuSampling | TimerKind::Animation)
            | Effect::SaveSettings(_)
            | Effect::CommitSettings { .. }
            | Effect::CancelCommit { .. }
            | Effect::CheckForUpdates { .. }
            | Effect::LaunchTaskManager
            | Effect::Quit => {}
        }
    }

    /// Opens the right-click menu. Menu handles exist only for this invocation.
    pub fn show_menu(&mut self, update_state: &UpdateMenuState) {
        let restore_pinned = self.flyout_pinned;
        self.flyout.hide();
        super::process::trim_working_set();
        let root = unsafe { CreatePopupMenu() };
        let display_menu = unsafe { CreatePopupMenu() };
        let theme_menu = unsafe { CreatePopupMenu() };
        let speed_menu = unsafe { CreatePopupMenu() };
        if root.is_null() || display_menu.is_null() || theme_menu.is_null() || speed_menu.is_null()
        {
            if !root.is_null() {
                let _ = unsafe { DestroyMenu(root) };
            }
            if !display_menu.is_null() {
                let _ = unsafe { DestroyMenu(display_menu) };
            }
            if !theme_menu.is_null() {
                let _ = unsafe { DestroyMenu(theme_menu) };
            }
            if !speed_menu.is_null() {
                let _ = unsafe { DestroyMenu(speed_menu) };
            }
            return;
        }

        let text = super::i18n::current().menu();
        append_checked(
            display_menu,
            COMMAND_DISPLAY_DOG,
            text.display_dog,
            self.display_mode == TrayDisplayMode::Dog,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_CPU,
            text.display_cpu,
            self.display_mode == TrayDisplayMode::Cpu,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_MEMORY,
            text.display_memory,
            self.display_mode == TrayDisplayMode::Memory,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_GPU,
            text.display_gpu,
            self.display_mode == TrayDisplayMode::Gpu,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_CLAUDE_5H,
            text.display_claude_5h,
            self.display_mode == TrayDisplayMode::Claude5h,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_CLAUDE_WEEK,
            text.display_claude_week,
            self.display_mode == TrayDisplayMode::ClaudeWeek,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_FABLE_WEEK,
            text.display_fable_week,
            self.display_mode == TrayDisplayMode::FableWeek,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_CODEX_5H,
            text.display_codex_5h,
            self.display_mode == TrayDisplayMode::Codex5h,
        );
        append_checked(
            display_menu,
            COMMAND_DISPLAY_CODEX_WEEK,
            text.display_codex_week,
            self.display_mode == TrayDisplayMode::CodexWeek,
        );
        append_checked(
            theme_menu,
            COMMAND_THEME_SYSTEM,
            text.theme_system,
            self.theme == ThemePreference::System,
        );
        append_checked(
            theme_menu,
            COMMAND_THEME_LIGHT,
            text.theme_light,
            self.theme == ThemePreference::Light,
        );
        append_checked(
            theme_menu,
            COMMAND_THEME_DARK,
            text.theme_dark,
            self.theme == ThemePreference::Dark,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_10,
            text.fps_10,
            self.fps_limit == FpsLimit::Fps10,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_20,
            text.fps_20,
            self.fps_limit == FpsLimit::Fps20,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_30,
            text.fps_30,
            self.fps_limit == FpsLimit::Fps30,
        );
        append_checked(
            speed_menu,
            COMMAND_FPS_40,
            text.fps_40,
            self.fps_limit == FpsLimit::Fps40,
        );

        append_submenu(root, display_menu, text.display);
        append_submenu(root, theme_menu, text.theme);
        append_submenu(root, speed_menu, text.animation_speed);
        let _ = unsafe { AppendMenuW(root, MF_SEPARATOR, 0, ptr::null()) };
        append_checked(
            root,
            COMMAND_TOGGLE_STARTUP,
            text.startup(self.startup_enabled),
            self.startup_enabled,
        );
        append_checked(
            root,
            COMMAND_TOGGLE_AUTO_UPDATE,
            text.auto_update(self.auto_update_enabled),
            self.auto_update_enabled,
        );
        append_checked(
            root,
            COMMAND_TOGGLE_PINNED_FLYOUT,
            text.pinned_flyout(self.flyout_pinned),
            self.flyout_pinned,
        );
        let _ = unsafe { AppendMenuW(root, MF_SEPARATOR, 0, ptr::null()) };
        append_action(root, COMMAND_RESCAN_MONTH_USAGE, text.rescan_month);
        append_update_menu(root, update_state, text);
        let _ = unsafe { AppendMenuW(root, MF_SEPARATOR, 0, ptr::null()) };
        append_action(root, COMMAND_ABOUT, text.about);
        append_checked(root, COMMAND_EXIT, text.exit, false);

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
        if restore_pinned {
            self.show_pinned_flyout();
        }
    }

    #[must_use]
    pub const fn is_context_menu_notification(notification: u32) -> bool {
        // NOTIFYICON_VERSION_4 delivers WM_CONTEXTMENU for the tray menu. Handling
        // WM_RBUTTONUP as well opens the menu twice and can treat the button-up
        // as a click on the item under the cursor (spurious startup toggles).
        notification == WM_CONTEXTMENU
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
    pub const fn is_pin_toggle_notification(notification: u32) -> bool {
        notification == NIN_SELECT || notification == NIN_KEYSELECT
    }

    #[must_use]
    pub const fn is_popup_open_notification(notification: u32) -> bool {
        notification == NIN_POPUPOPEN
    }

    #[must_use]
    pub const fn is_popup_close_notification(notification: u32) -> bool {
        notification == NIN_POPUPCLOSE
    }

    pub fn toggle_pinned_flyout(&mut self) {
        self.flyout_pinned = !self.flyout_pinned;
        let _ = super::registry::save_pinned_flyout(self.flyout_pinned);
        self.flyout.set_pinned(self.flyout_pinned);
        if self.flyout_pinned {
            self.show_pinned_flyout();
        } else {
            self.flyout.hide();
            super::process::trim_working_set();
        }
    }

    fn show_pinned_flyout(&mut self) {
        if self.last_icon.is_some() {
            self.flyout.set_pinned(true);
            self.flyout.show_near_icon(self.hwnd);
        }
    }

    pub fn handle_hover(&mut self, notification: u32) {
        if Self::is_popup_close_notification(notification) {
            self.flyout.hide_unless_pinned();
            if !self.flyout.is_pinned() {
                super::process::trim_working_set();
            }
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
            display_mode: crate::core::TrayDisplayMode::Dog,
            tooltip: String::new(),
            cpu_sparkline: crate::core::Sparkline::new(),
            memory_sparkline: crate::core::Sparkline::new(),
            gpu_sparkline: crate::core::Sparkline::new(),
            cpu_breakdown: None,
            memory: None,
            storage: None,
            gpu: None,
            usage: crate::core::UsageSnapshot::default(),
            process: None,
        });
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
        self.added = false;
    }

    fn icon_handle(&mut self, icon: &TrayIcon) -> HICON {
        if icon.display_mode.uses_animation() {
            return self.icons.icon(icon.theme, icon.frame);
        }
        let Some(glyph) = format_tray_glyph(icon.display_mode, icon.metrics(), unix_now_ms())
        else {
            return self.icons.icon(icon.theme, icon.frame);
        };
        let key = (icon.theme, format!("{}:{}", glyph.tag, glyph.value));
        if self.text_key.as_ref() == Some(&key) {
            if let Some(icon) = self.text_icon.as_ref() {
                return icon.raw();
            }
        }
        match GeneratedIcon::from_glyph(icon.theme, &glyph) {
            Ok(generated) => {
                let handle = generated.raw();
                self.text_icon = Some(generated);
                self.text_key = Some(key);
                handle
            }
            Err(_) => self.icons.icon(icon.theme, icon.frame),
        }
    }

    fn notification_data(&mut self, icon: &TrayIcon) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MESSAGE,
            hIcon: self.icon_handle(icon),
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

    pub fn notify_update_result(&mut self, state: &UpdateMenuState, notify_always: bool) {
        if let Some(body) = update_balloon_text(super::i18n::current().menu(), state, notify_always)
        {
            self.show_balloon("RunDog", &body);
        }
    }

    pub fn notify_month_rescan_started(&mut self) {
        let text = super::i18n::current().menu();
        self.show_balloon("RunDog", text.balloon_rescan_started);
    }

    pub fn notify_month_rescan_finished(&mut self) {
        self.show_balloon(
            "RunDog",
            super::i18n::current().menu().balloon_rescan_finished,
        );
    }

    fn show_balloon(&mut self, title: &str, body: &str) {
        if !self.added {
            return;
        }
        let Some(icon) = self.last_icon.clone() else {
            return;
        };
        let mut data = self.notification_data(&icon);
        data.uFlags |= NIF_INFO;
        data.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND;
        copy_utf16(title, &mut data.szInfoTitle);
        copy_utf16(body, &mut data.szInfo);
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn remember(&mut self, icon: &TrayIcon) {
        let display_changed = self.last_icon.as_ref().is_none_or(|previous| {
            previous.theme != icon.theme
                || previous.display_mode != icon.display_mode
                || previous.tooltip != icon.tooltip
                || previous.cpu_sparkline != icon.cpu_sparkline
                || previous.memory_sparkline != icon.memory_sparkline
                || previous.gpu_sparkline != icon.gpu_sparkline
                || previous.cpu_breakdown != icon.cpu_breakdown
                || previous.memory != icon.memory
                || previous.storage != icon.storage
                || previous.gpu != icon.gpu
                || previous.usage != icon.usage
                || previous.process != icon.process
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

fn append_update_menu(menu: HMENU, state: &UpdateMenuState, text: super::i18n::MenuText) {
    match state {
        UpdateMenuState::Idle => {
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, text.check_updates);
        }
        UpdateMenuState::Checking => {
            append_disabled(menu, text.checking_updates);
        }
        UpdateMenuState::Current => {
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, text.check_updates);
            append_disabled(menu, text.up_to_date);
        }
        UpdateMenuState::Available { version } => {
            append_action(
                menu,
                COMMAND_INSTALL_UPDATE,
                &text.with_version(text.install_update, version),
            );
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, text.check_again);
        }
        UpdateMenuState::Downloading { version } => {
            append_disabled(menu, &text.with_version(text.downloading, version));
        }
        UpdateMenuState::Launching => {
            append_disabled(menu, text.starting_installer);
        }
        UpdateMenuState::Failed => {
            append_action(menu, COMMAND_CHECK_FOR_UPDATES, text.retry_update);
            append_disabled(menu, text.update_failed);
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
fn update_balloon_text(
    text: super::i18n::MenuText,
    state: &UpdateMenuState,
    notify_always: bool,
) -> Option<String> {
    match state {
        UpdateMenuState::Current if notify_always => Some(text.balloon_up_to_date.to_owned()),
        UpdateMenuState::Available { version } => {
            Some(text.with_version(text.balloon_available, version))
        }
        UpdateMenuState::Failed if notify_always => Some(text.balloon_check_failed.to_owned()),
        _ => None,
    }
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn copy_utf16(value: &str, dest: &mut [u16]) {
    if dest.is_empty() {
        return;
    }
    dest.fill(0);
    let limit = dest.len() - 1;
    for (slot, utf16) in dest.iter_mut().take(limit).zip(value.encode_utf16()) {
        *slot = utf16;
    }
}

#[must_use]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        event_for_command, update_balloon_text, TrayAdapter, UpdateMenuState, COMMAND_ABOUT,
        COMMAND_CHECK_FOR_UPDATES, COMMAND_DISPLAY_CLAUDE_5H, COMMAND_DISPLAY_CODEX_WEEK,
        COMMAND_DISPLAY_CPU, COMMAND_DISPLAY_FABLE_WEEK, COMMAND_EXIT, COMMAND_FPS_40,
        COMMAND_THEME_DARK, COMMAND_TOGGLE_AUTO_UPDATE, COMMAND_TOGGLE_STARTUP,
    };
    use crate::{
        application::Event,
        core::{FpsLimit, ThemePreference, TrayDisplayMode},
        windows::i18n::UiLanguage,
    };
    use windows_sys::Win32::UI::Shell::NIN_SELECT;
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
            event_for_command(COMMAND_DISPLAY_CPU),
            Some(Event::SelectDisplayMode(TrayDisplayMode::Cpu))
        );
        assert_eq!(
            event_for_command(COMMAND_DISPLAY_CLAUDE_5H),
            Some(Event::SelectDisplayMode(TrayDisplayMode::Claude5h))
        );
        assert_eq!(
            event_for_command(COMMAND_DISPLAY_FABLE_WEEK),
            Some(Event::SelectDisplayMode(TrayDisplayMode::FableWeek))
        );
        assert_eq!(
            event_for_command(COMMAND_DISPLAY_CODEX_WEEK),
            Some(Event::SelectDisplayMode(TrayDisplayMode::CodexWeek))
        );
        assert_eq!(
            event_for_command(COMMAND_TOGGLE_STARTUP),
            Some(Event::ToggleStartup)
        );
        assert_eq!(
            event_for_command(COMMAND_TOGGLE_AUTO_UPDATE),
            Some(Event::ToggleAutoUpdate)
        );
        assert_eq!(event_for_command(COMMAND_EXIT), Some(Event::ExitRequested));
        assert_eq!(event_for_command(COMMAND_CHECK_FOR_UPDATES), None);
        assert_eq!(event_for_command(COMMAND_ABOUT), None);
        assert_eq!(event_for_command(0), None);
    }

    #[test]
    fn component_tray_notification_classifier_covers_click_kinds() {
        let v4_right_click = TrayAdapter::notification_code((1 << 16) | WM_RBUTTONUP);
        let v4_context_menu = TrayAdapter::notification_code((1 << 16) | WM_CONTEXTMENU);

        assert!(TrayAdapter::is_context_menu_notification(v4_context_menu));
        assert!(!TrayAdapter::is_context_menu_notification(v4_right_click));
        assert!(!TrayAdapter::is_context_menu_notification(515));
        assert!(TrayAdapter::is_pin_toggle_notification(NIN_SELECT));
        let v4_keyboard_select = TrayAdapter::notification_code((37 << 16) | (NIN_SELECT | 1));
        assert!(TrayAdapter::is_pin_toggle_notification(v4_keyboard_select));
        assert!(!TrayAdapter::is_pin_toggle_notification(515));
        assert!(!TrayAdapter::is_pin_toggle_notification(0));
        assert!(TrayAdapter::is_popup_open_notification(0x0406));
        assert!(TrayAdapter::is_popup_close_notification(0x0407));
        assert!(!TrayAdapter::is_popup_open_notification(0x0400));
    }

    #[test]
    fn component_english_menu_copy_and_update_balloons_cover_user_visible_states() {
        let en = UiLanguage::English.menu();
        assert_eq!(en.display, "Tray icon");
        assert_eq!(en.display_codex_week, "Codex week");
        assert_eq!(en.startup(true), "Launch at startup: On");
        assert_eq!(en.startup(false), "Launch at startup: Off");
        assert_eq!(en.auto_update(true), "Auto-update on startup: On");
        assert_eq!(en.auto_update(false), "Auto-update on startup: Off");
        assert_eq!(en.pinned_flyout(true), "Pin monitor card: On");
        assert_eq!(en.pinned_flyout(false), "Pin monitor card: Off");
        assert_eq!(en.about, "About");
        assert_eq!(
            update_balloon_text(en, &UpdateMenuState::Current, true).as_deref(),
            Some("RunDog is up to date.")
        );
        assert_eq!(
            update_balloon_text(en, &UpdateMenuState::Current, false),
            None
        );
        assert_eq!(
            update_balloon_text(
                en,
                &UpdateMenuState::Available {
                    version: "1.1.1".to_owned()
                },
                false
            )
            .as_deref(),
            Some("RunDog v1.1.1 is available.")
        );
        assert_eq!(
            update_balloon_text(en, &UpdateMenuState::Failed, true).as_deref(),
            Some("Could not check for updates.")
        );
        assert_eq!(update_balloon_text(en, &UpdateMenuState::Idle, true), None);
    }
}
