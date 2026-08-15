//! Win32 adapter. Platform calls stay in this module so component and
//! integration tests can exercise the rest of the crate without live OS state.

mod cpu;
mod icons;
mod registry;
mod tray;
mod update;

use std::ptr;

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{CreateMutexW, ReleaseMutex},
    },
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            GetWindowLongPtrW, KillTimer, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
            SetTimer, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, MSG, SW_SHOWNORMAL,
            WM_COMMAND, WM_DESTROY, WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMER, WNDCLASSW,
        },
    },
};

use crate::{
    application::{dispatch_and_execute, App, Effect, EffectPort, Event, TimerKind},
    core::{AppSettings, ResolvedTheme},
};

use self::{
    cpu::WindowsCpuSource,
    icons::IconFrames,
    tray::{
        event_for_command, TrayAdapter, COMMAND_CHECK_FOR_UPDATES, COMMAND_INSTALL_UPDATE,
        TRAY_CALLBACK_MESSAGE,
    },
    update::{UpdateController, UPDATE_REQUEST_EXIT_MESSAGE},
};

const WINDOW_CLASS_NAME: &str = "SystemExe.RunDog.MessageWindow";
const MUTEX_NAME: &str = "Local\\SystemExe.RunDog";
const TASKBAR_CREATED_MESSAGE: &str = "TaskbarCreated";
const TIMER_CPU: usize = 1;
const TIMER_ANIMATION: usize = 2;

/// Creates the hidden message window and runs the single-threaded tray loop.
pub fn run() -> Result<(), String> {
    let Some(_single_instance) = SingleInstance::acquire()? else {
        return Ok(());
    };

    let settings = registry::load_settings();
    let system_theme = registry::system_theme();
    let icons = IconFrames::load()?;
    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
    if hinstance.is_null() {
        return Err(last_error("GetModuleHandleW"));
    }

    let class_name = wide(WINDOW_CLASS_NAME);
    let window_class = WNDCLASSW {
        hInstance: hinstance,
        lpfnWndProc: Some(window_proc),
        lpszClassName: class_name.as_ptr(),
        ..WNDCLASSW::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(last_error("RegisterClassW"));
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            hinstance,
            ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err(last_error("CreateWindowExW"));
    }

    let mut context = Box::new(WindowContext::new(settings, system_theme, icons));
    let raw_context: *mut WindowContext = &mut *context;
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_context.cast::<()>() as isize);
    }
    context.platform.set_window(hwnd);
    let taskbar_message = wide(TASKBAR_CREATED_MESSAGE);
    context.taskbar_recreated_message = unsafe { RegisterWindowMessageW(taskbar_message.as_ptr()) };
    context.start();

    let loop_result = message_loop(hwnd);
    if context.app.snapshot().running {
        context.dispatch(Event::ExitRequested);
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }
    loop_result
}

fn message_loop(_hwnd: HWND) -> Result<(), String> {
    let mut message = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if status == -1 {
            return Err(last_error("GetMessageW"));
        }
        if status == 0 {
            return Ok(());
        }
        let _ = unsafe { TranslateMessage(&message) };
        unsafe {
            DispatchMessageW(&message);
        }
    }
}

struct WindowContext {
    app: App,
    cpu: WindowsCpuSource,
    platform: WindowsPlatform,
    updater: UpdateController,
    taskbar_recreated_message: u32,
}

impl WindowContext {
    fn new(settings: AppSettings, system_theme: ResolvedTheme, icons: IconFrames) -> Self {
        Self {
            app: App::new(settings, system_theme),
            cpu: WindowsCpuSource,
            platform: WindowsPlatform::new(icons, settings),
            updater: UpdateController::new(),
            taskbar_recreated_message: 0,
        }
    }

    fn start(&mut self) {
        for effect in self.app.start() {
            self.platform.apply(&effect);
        }
        self.updater.check_for_updates();
    }

    fn dispatch(&mut self, event: Event) {
        if matches!(&event, Event::ExitRequested) {
            self.updater.cancel();
        }
        dispatch_and_execute(&mut self.app, &mut self.platform, event);
    }
}

struct WindowsPlatform {
    hwnd: HWND,
    tray: TrayAdapter,
}

impl WindowsPlatform {
    fn new(icons: IconFrames, settings: AppSettings) -> Self {
        Self {
            hwnd: ptr::null_mut(),
            tray: TrayAdapter::new(
                icons,
                settings.theme,
                settings.fps_limit,
                settings.launch_at_startup,
            ),
        }
    }

    fn set_window(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        self.tray.set_window(hwnd);
    }
}

impl EffectPort for WindowsPlatform {
    fn apply(&mut self, effect: &Effect) {
        self.tray.apply(effect);
        match effect {
            Effect::SetTimer { kind, interval_ms } => {
                let _ = unsafe { SetTimer(self.hwnd, timer_id(*kind), *interval_ms, None) };
            }
            Effect::KillTimer(kind) => {
                let _ = unsafe { KillTimer(self.hwnd, timer_id(*kind)) };
            }
            Effect::SaveSettings(settings) => registry::save_settings(*settings),
            Effect::LaunchTaskManager => launch_task_manager(),
            Effect::Quit => unsafe { PostQuitMessage(0) },
            Effect::AddTray(_)
            | Effect::ModifyTray(_)
            | Effect::RemoveTray
            | Effect::SetThemeMenu(_)
            | Effect::SetFpsMenu(_)
            | Effect::SetStartupMenu(_)
            | Effect::RequestStartup(_) => {}
        }
    }

    fn set_startup(&mut self, enabled: bool) -> bool {
        registry::set_launch_at_startup(enabled)
    }
}

fn timer_id(kind: TimerKind) -> usize {
    match kind {
        TimerKind::CpuSampling => TIMER_CPU,
        TimerKind::Animation => TIMER_ANIMATION,
    }
}

fn launch_task_manager() {
    let verb = wide("open");
    let executable = wide("taskmgr.exe");
    let _ = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            executable.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return 0;
    }

    let context = unsafe { context_from_window(hwnd) };
    let Some(context) = context else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };

    if message == WM_TIMER {
        match wparam {
            TIMER_CPU => {
                if let Some(sample) =
                    crate::application::CpuSource::read_system_times(&mut context.cpu)
                {
                    context.dispatch(Event::CpuSample(sample));
                }
            }
            TIMER_ANIMATION => context.dispatch(Event::AnimationTimerElapsed),
            _ => {}
        }
        return 0;
    }

    if message == context.taskbar_recreated_message && context.taskbar_recreated_message != 0 {
        context.dispatch(Event::TaskbarRecreated);
        return 0;
    }

    if message == TRAY_CALLBACK_MESSAGE {
        let notification = TrayAdapter::notification_code(lparam as u32);
        if TrayAdapter::is_context_menu_notification(notification) {
            let update_state = context.updater.menu_state();
            context.platform.tray.show_menu(&update_state);
        } else if TrayAdapter::is_activation_notification(notification) {
            context.dispatch(Event::TrayActivated);
        }
        return 0;
    }

    if message == UPDATE_REQUEST_EXIT_MESSAGE {
        context.dispatch(Event::ExitRequested);
        return 0;
    }

    if message == WM_COMMAND {
        let command = (wparam & 0xFFFF) as u32;
        if command == COMMAND_CHECK_FOR_UPDATES {
            context.updater.check_for_updates();
        } else if command == COMMAND_INSTALL_UPDATE {
            context.updater.install_available(hwnd);
        } else if let Some(event) = event_for_command(command) {
            context.dispatch(event);
        }
        return 0;
    }

    if message == WM_SETTINGCHANGE || message == WM_THEMECHANGED {
        context.dispatch(Event::SystemThemeChanged(registry::system_theme()));
        return 0;
    }

    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// SAFETY: `run` stores a valid `Box<WindowContext>` pointer in this window's
/// user-data slot before entering the message loop, and clears it before the
/// context is dropped.
unsafe fn context_from_window<'a>(hwnd: HWND) -> Option<&'a mut WindowContext> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowContext;
    unsafe { pointer.as_mut() }
}

struct SingleInstance(HANDLE);

impl SingleInstance {
    fn acquire() -> Result<Option<Self>, String> {
        let name = wide(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            return Err(last_error("CreateMutexW"));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = unsafe { ReleaseMutex(self.0) };
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[must_use]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[must_use]
fn last_error(operation: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("{operation} failed with Win32 error {code}")
}
