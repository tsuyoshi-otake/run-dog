//! Hover flyout styled as a compact system-monitor card.
//!
//! The shell tooltip is text-only, so `NIN_POPUPOPEN` owns a `WS_EX_NOACTIVATE`
//! popup instead. All GDI objects live only for one paint.

use std::{mem::size_of, ptr, sync::Once};

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND},
            Gdi::{
                BeginPaint, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush,
                DeleteObject, DrawTextW, Ellipse, EndPaint, FillRect, GetDC, GetDeviceCaps,
                GetStockObject, InvalidateRect, LineTo, MoveToEx, Polygon, Polyline, ReleaseDC,
                RoundRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY,
                DEFAULT_CHARSET, DEFAULT_GUI_FONT, DT_END_ELLIPSIS, DT_NOPREFIX, DT_RIGHT,
                DT_SINGLELINE, DT_VCENTER, FW_NORMAL, FW_SEMIBOLD, LOGPIXELSX, NULL_BRUSH,
                NULL_PEN, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
                GetSystemMetrics, GetWindowLongPtrW, IsWindow, IsWindowVisible, RegisterClassW,
                SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_DROPSHADOW, GWLP_USERDATA,
                HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
                SW_SHOWNOACTIVATE, WM_DESTROY, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSW,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
};

use crate::{
    application::TrayIcon,
    core::{
        local_hms, local_ymd, LimitWindow, MemoryStatus, ProviderUsage, ResolvedTheme, Sparkline,
        StorageStatus, SPARKLINE_CAPACITY,
    },
};

const CLASS_NAME: &str = "SystemExe.RunDog.HoverFlyout";
const ICON_ID: u32 = 1;
const MA_NOACTIVATE: LRESULT = 3;
const CARD_WIDTH: i32 = 320;
const CARD_HEIGHT: i32 = 272;
const CARD_USAGE_HEIGHT: i32 = 188;

static REGISTER_CLASS: Once = Once::new();

pub struct HoverFlyout {
    hwnd: HWND,
    state: Option<TrayIcon>,
}

impl HoverFlyout {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            hwnd: ptr::null_mut(),
            state: None,
        }
    }

    pub fn set_state(&mut self, icon: &TrayIcon) {
        self.state = Some(icon.clone());
        if self.is_visible() {
            let _ = unsafe { InvalidateRect(self.hwnd, ptr::null(), 1) };
        }
    }

    pub fn show_near_icon(&mut self, owner: HWND) {
        register_class();
        if self.hwnd.is_null() {
            self.hwnd = create_flyout_window(owner, self);
            if self.hwnd.is_null() {
                return;
            }
        }
        let dpi = window_dpi(self.hwnd);
        let (width, height) = window_size(dpi);
        let (x, y) = position_near_icon(owner, width, height);
        let _ = unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        };
        apply_rounded_chrome(self.hwnd, width, height, dpi);
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
        let _ = unsafe { InvalidateRect(self.hwnd, ptr::null(), 1) };
    }

    pub fn hide(&mut self) {
        if !self.hwnd.is_null() {
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    pub fn destroy(&mut self) {
        if !self.hwnd.is_null() && unsafe { IsWindow(self.hwnd) } != 0 {
            let _ = unsafe { SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0) };
            let _ = unsafe { DestroyWindow(self.hwnd) };
        }
        self.hwnd = ptr::null_mut();
        self.state = None;
    }

    #[must_use]
    pub fn is_visible(&self) -> bool {
        !self.hwnd.is_null() && unsafe { IsWindowVisible(self.hwnd) } != 0
    }
}

impl Drop for HoverFlyout {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn register_class() {
    REGISTER_CLASS.call_once(|| {
        let class_name = wide(CLASS_NAME);
        let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
        let class = WNDCLASSW {
            style: CS_DROPSHADOW,
            hInstance: hinstance,
            lpfnWndProc: Some(flyout_proc),
            lpszClassName: class_name.as_ptr(),
            ..WNDCLASSW::default()
        };
        let _ = unsafe { RegisterClassW(&class) };
    });
}

fn create_flyout_window(owner: HWND, flyout: *mut HoverFlyout) -> HWND {
    let class_name = wide(CLASS_NAME);
    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name.as_ptr(),
            ptr::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            owner,
            ptr::null_mut(),
            hinstance,
            ptr::null(),
        )
    };
    if !hwnd.is_null() {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, flyout as isize);
        }
    }
    hwnd
}

fn apply_rounded_chrome(hwnd: HWND, width: i32, height: i32, dpi: i32) {
    let preference = DWMWCP_ROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            ptr::from_ref(&preference).cast(),
            size_of::<i32>() as u32,
        )
    };
    let radius = px(12, dpi);
    let region = unsafe { CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2) };
    if !region.is_null() {
        let _ = unsafe { SetWindowRgn(hwnd, region, 1) };
    }
}

fn position_near_icon(owner: HWND, width: i32, height: i32) -> (i32, i32) {
    let ident = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: owner,
        uID: ICON_ID,
        guidItem: GUID {
            data1: 0,
            data2: 0,
            data3: 0,
            data4: [0; 8],
        },
    };
    let mut icon = RECT::default();
    let have_icon = unsafe { Shell_NotifyIconGetRect(&ident, &mut icon) } >= 0;
    let mut origin = POINT::default();
    if have_icon {
        origin.x = (icon.left + icon.right) / 2;
        origin.y = icon.top;
    } else if unsafe { GetCursorPos(&mut origin) } == 0 {
        origin.x = 0;
        origin.y = 0;
    }

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let mut x = origin.x - width / 2;
    let mut y = origin.y - height - 8;
    if y < 0 {
        y = if have_icon {
            icon.bottom + 8
        } else {
            origin.y + 16
        };
    }
    if x < 8 {
        x = 8;
    }
    if x + width > screen_w - 8 {
        x = screen_w - width - 8;
    }
    if y + height > screen_h - 8 {
        y = screen_h - height - 8;
    }
    (x.max(0), y.max(0))
}

fn window_size(dpi: i32) -> (i32, i32) {
    (
        px(CARD_WIDTH, dpi),
        px(CARD_HEIGHT + CARD_USAGE_HEIGHT, dpi),
    )
}

fn window_dpi(hwnd: HWND) -> i32 {
    let hdc = unsafe { GetDC(hwnd) };
    if hdc.is_null() {
        return 96;
    }
    let dpi = unsafe { GetDeviceCaps(hdc, LOGPIXELSX as i32) };
    let _ = unsafe { ReleaseDC(hwnd, hdc) };
    if dpi <= 0 {
        96
    } else {
        dpi
    }
}

const fn px(value: i32, dpi: i32) -> i32 {
    (value * dpi) / 96
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

struct Palette {
    background: COLORREF,
    text: COLORREF,
    muted: COLORREF,
    separator: COLORREF,
    accent: COLORREF,
    accent_fill: COLORREF,
    track: COLORREF,
}

impl Palette {
    fn for_theme(theme: ResolvedTheme) -> Self {
        if theme.is_light() {
            Self {
                background: rgb(244, 246, 250),
                text: rgb(28, 32, 40),
                muted: rgb(92, 100, 114),
                separator: rgb(214, 218, 226),
                accent: rgb(243, 112, 33),
                accent_fill: rgb(232, 150, 96),
                track: rgb(226, 230, 236),
            }
        } else {
            Self {
                background: rgb(38, 44, 56),
                text: rgb(245, 247, 250),
                muted: rgb(156, 166, 182),
                separator: rgb(58, 66, 80),
                accent: rgb(243, 112, 33),
                accent_fill: rgb(168, 78, 28),
                track: rgb(52, 60, 74),
            }
        }
    }
}

struct Layout {
    dpi: i32,
    pad: i32,
    icon: i32,
    gap: i32,
    title_h: i32,
    detail_h: i32,
    spark_w: i32,
    spark_h: i32,
    bar_h: i32,
}

impl Layout {
    fn new(dpi: i32) -> Self {
        Self {
            dpi,
            pad: px(16, dpi),
            icon: px(20, dpi),
            gap: px(12, dpi),
            title_h: px(18, dpi),
            detail_h: px(15, dpi),
            spark_w: px(88, dpi),
            spark_h: px(36, dpi),
            bar_h: px(6, dpi),
        }
    }
}

unsafe extern "system" fn flyout_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE {
        return MA_NOACTIVATE;
    }
    if message == WM_DESTROY {
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        return 0;
    }
    if message == WM_PAINT {
        paint(hwnd);
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn paint(hwnd: HWND) {
    let flyout = unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HoverFlyout;
        pointer.as_ref()
    };
    let Some(flyout) = flyout else {
        return;
    };
    let Some(state) = flyout.state.as_ref() else {
        return;
    };

    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.is_null() {
        return;
    }

    let dpi = unsafe { GetDeviceCaps(hdc, LOGPIXELSX as i32) };
    let dpi = if dpi <= 0 { 96 } else { dpi };
    let palette = Palette::for_theme(state.theme);
    let layout = Layout::new(dpi);
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut client) };

    let background = unsafe { CreateSolidBrush(palette.background) };
    if !background.is_null() {
        let _ = unsafe { FillRect(hdc, &client, background) };
        let _ = unsafe { DeleteObject(background) };
    }

    let title_font = create_font(-px(14, dpi), FW_SEMIBOLD as i32);
    let detail_font = create_font(-px(11, dpi), FW_NORMAL as i32);
    let _ = unsafe { SetBkMode(hdc, TRANSPARENT as i32) };
    let previous_font = unsafe { SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT)) };

    let content_left = client.left + layout.pad + layout.icon + layout.gap;
    let content_right = client.right - layout.pad;
    let mut top = client.top + layout.pad;

    let cpu_bottom = top + layout.title_h + layout.detail_h * 3 + px(6, dpi);
    paint_cpu_row(
        hdc,
        RECT {
            left: client.left + layout.pad,
            top,
            right: content_right,
            bottom: cpu_bottom,
        },
        content_left,
        state,
        &palette,
        &layout,
        title_font,
        detail_font,
    );
    top = cpu_bottom + px(8, dpi);
    draw_separator(hdc, content_left, content_right, top, palette.separator);
    top += px(9, dpi);

    let memory_bottom = top + layout.title_h + layout.detail_h * 3 + px(6, dpi);
    paint_memory_row(
        hdc,
        RECT {
            left: client.left + layout.pad,
            top,
            right: content_right,
            bottom: memory_bottom,
        },
        content_left,
        state,
        &palette,
        &layout,
        title_font,
        detail_font,
    );
    top = memory_bottom + px(8, dpi);
    draw_separator(hdc, content_left, content_right, top, palette.separator);
    top += px(9, dpi);

    let storage_bottom = top + layout.title_h + layout.detail_h + layout.bar_h + px(10, dpi);
    paint_storage_row(
        hdc,
        RECT {
            left: client.left + layout.pad,
            top,
            right: content_right,
            bottom: storage_bottom,
        },
        content_left,
        state.storage,
        &palette,
        &layout,
        title_font,
        detail_font,
    );
    top = storage_bottom + px(8, dpi);
    draw_separator(hdc, content_left, content_right, top, palette.separator);
    top += px(9, dpi);

    let claude_bottom = top + usage_block_height(&layout);
    paint_usage_row(
        hdc,
        RECT {
            left: client.left + layout.pad,
            top,
            right: content_right,
            bottom: claude_bottom,
        },
        content_left,
        "Claude",
        state.usage.claude,
        UsageMark::Claude,
        &palette,
        &layout,
        title_font,
        detail_font,
    );
    top = claude_bottom + px(8, dpi);
    draw_separator(hdc, content_left, content_right, top, palette.separator);
    top += px(9, dpi);

    let codex_bottom = top + usage_block_height(&layout);
    paint_usage_row(
        hdc,
        RECT {
            left: client.left + layout.pad,
            top,
            right: content_right,
            bottom: codex_bottom,
        },
        content_left,
        "Codex",
        state.usage.codex,
        UsageMark::Codex,
        &palette,
        &layout,
        title_font,
        detail_font,
    );

    if !previous_font.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_font) };
    }
    destroy_font(title_font);
    destroy_font(detail_font);
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

fn create_font(height: i32, weight: i32) -> windows_sys::Win32::Graphics::Gdi::HFONT {
    let face = wide("Segoe UI");
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            face.as_ptr(),
        )
    }
}

fn destroy_font(font: windows_sys::Win32::Graphics::Gdi::HFONT) {
    if !font.is_null() {
        let _ = unsafe { DeleteObject(font) };
    }
}

fn select_font(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    font: windows_sys::Win32::Graphics::Gdi::HFONT,
) {
    if !font.is_null() {
        let _ = unsafe { SelectObject(hdc, font) };
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_cpu_row(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    row: RECT,
    content_left: i32,
    state: &TrayIcon,
    palette: &Palette,
    layout: &Layout,
    title_font: windows_sys::Win32::Graphics::Gdi::HFONT,
    detail_font: windows_sys::Win32::Graphics::Gdi::HFONT,
) {
    draw_cpu_icon(
        hdc,
        RECT {
            left: row.left,
            top: row.top + px(2, layout.dpi),
            right: row.left + layout.icon,
            bottom: row.top + px(2, layout.dpi) + layout.icon,
        },
        palette.text,
        layout.dpi,
    );

    let spark = spark_rect(row, layout);
    draw_area_sparkline(hdc, spark, state.cpu_sparkline, palette, layout.dpi);

    let text_right = spark.left - px(10, layout.dpi);
    select_font(hdc, title_font);
    let _ = unsafe { SetTextColor(hdc, palette.text) };
    draw_text(
        hdc,
        RECT {
            left: content_left,
            top: row.top,
            right: text_right,
            bottom: row.top + layout.title_h,
        },
        &format!("CPU: {}", format_percent(cpu_total(state))),
        0,
    );

    select_font(hdc, detail_font);
    let _ = unsafe { SetTextColor(hdc, palette.muted) };
    let mut detail_top = row.top + layout.title_h + px(2, layout.dpi);
    for line in cpu_details(state) {
        draw_text(
            hdc,
            RECT {
                left: content_left,
                top: detail_top,
                right: text_right,
                bottom: detail_top + layout.detail_h,
            },
            &line,
            0,
        );
        detail_top += layout.detail_h;
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_memory_row(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    row: RECT,
    content_left: i32,
    state: &TrayIcon,
    palette: &Palette,
    layout: &Layout,
    title_font: windows_sys::Win32::Graphics::Gdi::HFONT,
    detail_font: windows_sys::Win32::Graphics::Gdi::HFONT,
) {
    draw_memory_icon(
        hdc,
        RECT {
            left: row.left,
            top: row.top + px(2, layout.dpi),
            right: row.left + layout.icon,
            bottom: row.top + px(2, layout.dpi) + layout.icon,
        },
        palette.text,
        layout.dpi,
    );

    let spark = spark_rect(row, layout);
    draw_area_sparkline(hdc, spark, state.memory_sparkline, palette, layout.dpi);
    let text_right = spark.left - px(10, layout.dpi);

    select_font(hdc, title_font);
    let _ = unsafe { SetTextColor(hdc, palette.text) };
    draw_text(
        hdc,
        RECT {
            left: content_left,
            top: row.top,
            right: text_right,
            bottom: row.top + layout.title_h,
        },
        &format!(
            "Memory: {}",
            format_percent(state.memory.and_then(MemoryStatus::usage_percent))
        ),
        0,
    );

    select_font(hdc, detail_font);
    let _ = unsafe { SetTextColor(hdc, palette.muted) };
    let mut detail_top = row.top + layout.title_h + px(2, layout.dpi);
    for line in memory_details(state.memory) {
        draw_text(
            hdc,
            RECT {
                left: content_left,
                top: detail_top,
                right: text_right,
                bottom: detail_top + layout.detail_h,
            },
            &line,
            0,
        );
        detail_top += layout.detail_h;
    }
}

fn spark_rect(row: RECT, layout: &Layout) -> RECT {
    RECT {
        left: row.right - layout.spark_w,
        top: row.top + ((row.bottom - row.top) - layout.spark_h) / 2,
        right: row.right,
        bottom: row.top + ((row.bottom - row.top) - layout.spark_h) / 2 + layout.spark_h,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_storage_row(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    row: RECT,
    content_left: i32,
    storage: Option<StorageStatus>,
    palette: &Palette,
    layout: &Layout,
    title_font: windows_sys::Win32::Graphics::Gdi::HFONT,
    detail_font: windows_sys::Win32::Graphics::Gdi::HFONT,
) {
    draw_storage_icon(
        hdc,
        RECT {
            left: row.left,
            top: row.top + px(2, layout.dpi),
            right: row.left + layout.icon,
            bottom: row.top + px(2, layout.dpi) + layout.icon,
        },
        palette.text,
        layout.dpi,
    );

    select_font(hdc, title_font);
    let _ = unsafe { SetTextColor(hdc, palette.text) };
    draw_text(
        hdc,
        RECT {
            left: content_left,
            top: row.top,
            right: row.right,
            bottom: row.top + layout.title_h,
        },
        &format!(
            "Storage: {} used",
            format_percent(storage.and_then(StorageStatus::used_percent))
        ),
        0,
    );

    select_font(hdc, detail_font);
    let _ = unsafe { SetTextColor(hdc, palette.muted) };
    let detail_top = row.top + layout.title_h + px(2, layout.dpi);
    draw_text(
        hdc,
        RECT {
            left: content_left,
            top: detail_top,
            right: row.right,
            bottom: detail_top + layout.detail_h,
        },
        &storage_capacity(storage),
        0,
    );

    let bar_top = detail_top + layout.detail_h + px(6, layout.dpi);
    draw_progress_bar(
        hdc,
        RECT {
            left: content_left,
            top: bar_top,
            right: row.right,
            bottom: bar_top + layout.bar_h,
        },
        storage.and_then(StorageStatus::used_percent).unwrap_or(0.0),
        palette,
        layout.dpi,
    );
}

#[derive(Clone, Copy)]
enum UsageMark {
    Claude,
    Codex,
}

#[allow(clippy::too_many_arguments)]
fn paint_usage_row(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    row: RECT,
    content_left: i32,
    title: &str,
    usage: ProviderUsage,
    mark: UsageMark,
    palette: &Palette,
    layout: &Layout,
    title_font: windows_sys::Win32::Graphics::Gdi::HFONT,
    detail_font: windows_sys::Win32::Graphics::Gdi::HFONT,
) {
    let icon = RECT {
        left: row.left,
        top: row.top + px(2, layout.dpi),
        right: row.left + layout.icon,
        bottom: row.top + px(2, layout.dpi) + layout.icon,
    };
    match mark {
        UsageMark::Claude => draw_claude_icon(hdc, icon, palette.text, layout.dpi),
        UsageMark::Codex => draw_codex_icon(hdc, icon, palette.text, layout.dpi),
    }

    let heading = match usage.plan_label() {
        Some(plan) => format!("{title} {plan}"),
        None => title.to_owned(),
    };
    select_font(hdc, title_font);
    let _ = unsafe { SetTextColor(hdc, palette.text) };
    draw_text(
        hdc,
        RECT {
            left: content_left,
            top: row.top,
            right: row.right,
            bottom: row.top + layout.title_h,
        },
        &heading,
        0,
    );
    draw_text(
        hdc,
        RECT {
            left: content_left,
            top: row.top,
            right: row.right,
            bottom: row.top + layout.title_h,
        },
        &format!(
            "Today {}",
            format_usd(
                usage.today_cents,
                usage.month_cents == 0 && usage.today_cents == 0
            )
        ),
        DT_RIGHT,
    );

    select_font(hdc, detail_font);
    let _ = unsafe { SetTextColor(hdc, palette.muted) };
    let mut metric_top = row.top + layout.title_h + px(2, layout.dpi);
    metric_top = paint_limit_metric(
        hdc,
        RECT {
            left: content_left,
            top: metric_top,
            right: row.right,
            bottom: metric_top + layout.detail_h + layout.bar_h,
        },
        "5h",
        usage.session_window(),
        palette,
        layout,
    );
    metric_top = paint_limit_metric(
        hdc,
        RECT {
            left: content_left,
            top: metric_top,
            right: row.right,
            bottom: metric_top + layout.detail_h + layout.bar_h,
        },
        "7d",
        usage.weekly_window(),
        palette,
        layout,
    );

    draw_text(
        hdc,
        RECT {
            left: content_left,
            top: metric_top,
            right: row.right,
            bottom: metric_top + layout.detail_h,
        },
        &format!(
            "Month {}",
            format_usd(usage.month_cents, usage.month_cents == 0)
        ),
        0,
    );
}

fn usage_block_height(layout: &Layout) -> i32 {
    let metric = layout.detail_h + layout.bar_h + px(3, layout.dpi);
    layout.title_h + metric * 2 + layout.detail_h + px(2, layout.dpi)
}

fn paint_limit_metric(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    name: &str,
    window: Option<LimitWindow>,
    palette: &Palette,
    layout: &Layout,
) -> i32 {
    let label = match window {
        Some(window) => {
            let reset = format_reset(window);
            if reset.is_empty() {
                format!("{name}: {:.0}%", window.used_percent())
            } else {
                format!("{name}: {:.0}%  {reset}", window.used_percent())
            }
        }
        None => format!("{name}: —"),
    };
    let text_bottom = rect.top + layout.detail_h;
    draw_text(
        hdc,
        RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: text_bottom,
        },
        &label,
        0,
    );
    let bar = RECT {
        left: rect.left,
        top: text_bottom + px(1, layout.dpi),
        right: rect.right,
        bottom: text_bottom + px(1, layout.dpi) + layout.bar_h,
    };
    draw_progress_bar(
        hdc,
        bar,
        window.map(LimitWindow::used_percent).unwrap_or(0.0),
        palette,
        layout.dpi,
    );
    bar.bottom + px(3, layout.dpi)
}

fn format_reset(window: LimitWindow) -> String {
    if window.resets_at_ms == 0 {
        return String::new();
    }
    let bias = super::usage::timezone_bias_minutes();
    if window.window_minutes >= 1_440 {
        let (_, month, day) = local_ymd(window.resets_at_ms, bias);
        format!("{month:02}-{day:02}")
    } else {
        let (hour, minute) = local_hms(window.resets_at_ms, bias);
        format!("{hour:02}:{minute:02}")
    }
}

fn format_usd(cents: u32, empty: bool) -> String {
    if empty {
        "—".to_owned()
    } else {
        format!("${}.{:02}", cents / 100, cents % 100)
    }
}

fn draw_claude_icon(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    dpi: i32,
) {
    let cx = (rect.left + rect.right) as f32 / 2.0;
    let cy = (rect.top + rect.bottom) as f32 / 2.0;
    let arm = px(6, dpi) as f32;
    let width = px(2, dpi).max(1);
    for index in 0..3 {
        let angle = index as f32 * std::f32::consts::PI / 3.0;
        let dx = arm * angle.cos();
        let dy = arm * angle.sin();
        stroke_line(
            hdc,
            (cx - dx).round() as i32,
            (cy - dy).round() as i32,
            (cx + dx).round() as i32,
            (cy + dy).round() as i32,
            color,
            width,
        );
    }
}

fn draw_codex_icon(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    dpi: i32,
) {
    let cx = (rect.left + rect.right) as f32 / 2.0;
    let cy = (rect.top + rect.bottom) as f32 / 2.0;
    let radius = px(7, dpi) as f32;
    let mut points = [POINT { x: 0, y: 0 }; 6];
    for (index, point) in points.iter_mut().enumerate() {
        let angle = std::f32::consts::FRAC_PI_6 + index as f32 * std::f32::consts::FRAC_PI_3;
        *point = POINT {
            x: (cx + radius * angle.cos()).round() as i32,
            y: (cy + radius * angle.sin()).round() as i32,
        };
    }
    stroke_polygon(hdc, &points, color, px(2, dpi).max(1));
}

fn stroke_polygon(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    points: &[POINT],
    color: COLORREF,
    width: i32,
) {
    if points.len() < 3 {
        return;
    }
    let pen = unsafe { CreatePen(PS_SOLID, width, color) };
    if pen.is_null() {
        return;
    }
    let previous_pen = unsafe { SelectObject(hdc, pen) };
    let previous_brush = unsafe { SelectObject(hdc, GetStockObject(NULL_BRUSH)) };
    let _ = unsafe { Polygon(hdc, points.as_ptr(), points.len() as i32) };
    if !previous_pen.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_pen) };
    }
    if !previous_brush.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_brush) };
    }
    let _ = unsafe { DeleteObject(pen) };
}

fn cpu_total(state: &TrayIcon) -> Option<f32> {
    state
        .cpu_breakdown
        .map(|breakdown| breakdown.total.value())
        .or_else(|| parse_tooltip_percent(state.tooltip.lines().next()))
}

fn cpu_details(state: &TrayIcon) -> [String; 3] {
    match state.cpu_breakdown {
        Some(breakdown) => [
            format!("System: {}", format_percent(Some(breakdown.system.value()))),
            format!("User: {}", format_percent(Some(breakdown.user.value()))),
            format!("Idle: {}", format_percent(Some(breakdown.idle.value()))),
        ],
        None => [
            "System: --.-%".to_owned(),
            "User: --.-%".to_owned(),
            "Idle: --.-%".to_owned(),
        ],
    }
}

fn memory_details(memory: Option<MemoryStatus>) -> [String; 3] {
    let used = memory.and_then(MemoryStatus::used_bytes);
    let available = memory.map(|status| status.available_bytes);
    [
        format!("In use: {}", format_bytes_or_unknown(used)),
        format!("Available: {}", format_bytes_or_unknown(available)),
        format!(
            "Committed: {}",
            format_percent(memory.and_then(MemoryStatus::commit_percent))
        ),
    ]
}

fn storage_capacity(storage: Option<StorageStatus>) -> String {
    match storage.and_then(|status| status.used_bytes().map(|used| (used, status.total_bytes))) {
        Some((used, total)) => format!("{} / {}", format_bytes(used, 2), format_bytes(total, 2)),
        None => "-- / --".to_owned(),
    }
}

fn parse_tooltip_percent(line: Option<&str>) -> Option<f32> {
    line.and_then(|line| line.strip_prefix("CPU: "))
        .and_then(|value| value.trim_end_matches('%').parse().ok())
}

fn format_percent(value: Option<f32>) -> String {
    match value {
        Some(percent) => format!("{percent:.1}%"),
        None => "--.-%".to_owned(),
    }
}

fn format_bytes_or_unknown(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "--".to_owned(), |value| format_bytes(value, 1))
}

fn format_bytes(bytes: u64, decimals: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let value = bytes as f64;
    if value >= TIB {
        format!("{:.decimals$} TB", value / TIB)
    } else if value >= GIB {
        format!("{:.decimals$} GB", value / GIB)
    } else if value >= MIB {
        format!("{:.decimals$} MB", value / MIB)
    } else {
        format!("{:.decimals$} KB", value / KIB)
    }
}

fn draw_text(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    mut rect: RECT,
    text: &str,
    extra_format: u32,
) {
    let wide = wide(text);
    let _ = unsafe {
        DrawTextW(
            hdc,
            wide.as_ptr(),
            (wide.len() - 1) as i32,
            &mut rect,
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS | extra_format,
        )
    };
}

fn draw_separator(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    left: i32,
    right: i32,
    y: i32,
    color: COLORREF,
) {
    stroke_line(hdc, left, y, right, y, color, 1);
}

fn draw_progress_bar(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    percent: f32,
    palette: &Palette,
    dpi: i32,
) {
    let radius = px(3, dpi);
    fill_round_rect(hdc, rect, palette.track, radius);
    let width = (rect.right - rect.left).max(1);
    let filled = ((width as f32) * percent.clamp(0.0, 100.0) / 100.0).round() as i32;
    if filled <= 0 {
        return;
    }
    fill_round_rect(
        hdc,
        RECT {
            left: rect.left,
            top: rect.top,
            right: (rect.left + filled).max(rect.left + radius),
            bottom: rect.bottom,
        },
        palette.accent,
        radius,
    );
}

fn draw_area_sparkline(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    sparkline: Sparkline,
    palette: &Palette,
    dpi: i32,
) {
    let (values, len) = sparkline.copy_points();
    if len == 0 {
        stroke_line(
            hdc,
            rect.left,
            rect.bottom - 1,
            rect.right - 1,
            rect.bottom - 1,
            palette.separator,
            1,
        );
        return;
    }

    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let last = (len - 1).max(1);
    let mut points = [POINT { x: 0, y: 0 }; SPARKLINE_CAPACITY + 3];
    for index in 0..len {
        let x = rect.left + (index as i32 * (width - 1)) / last as i32;
        let y = rect.bottom - 1 - (i32::from(values[index]) * (height - 1)) / 100;
        points[index] = POINT { x, y };
    }
    points[len] = POINT {
        x: points[len - 1].x,
        y: rect.bottom - 1,
    };
    points[len + 1] = POINT {
        x: points[0].x,
        y: rect.bottom - 1,
    };

    let fill = unsafe { CreateSolidBrush(palette.accent_fill) };
    let null_pen = unsafe { GetStockObject(NULL_PEN) };
    if !fill.is_null() {
        let previous_brush = unsafe { SelectObject(hdc, fill) };
        let previous_pen = unsafe { SelectObject(hdc, null_pen) };
        let _ = unsafe { Polygon(hdc, points.as_ptr(), (len + 2) as i32) };
        if !previous_brush.is_null() {
            let _ = unsafe { SelectObject(hdc, previous_brush) };
        }
        if !previous_pen.is_null() {
            let _ = unsafe { SelectObject(hdc, previous_pen) };
        }
        let _ = unsafe { DeleteObject(fill) };
    }

    let stroke = px(2, dpi).max(1);
    let pen = unsafe { CreatePen(PS_SOLID, stroke, palette.accent) };
    if pen.is_null() {
        return;
    }
    let previous = unsafe { SelectObject(hdc, pen) };
    let _ = unsafe { Polyline(hdc, points.as_ptr(), len as i32) };
    if !previous.is_null() {
        let _ = unsafe { SelectObject(hdc, previous) };
    }
    let _ = unsafe { DeleteObject(pen) };
}

fn draw_cpu_icon(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    dpi: i32,
) {
    let inset = px(3, dpi);
    let body = RECT {
        left: rect.left + inset,
        top: rect.top + inset,
        right: rect.right - inset,
        bottom: rect.bottom - inset,
    };
    stroke_round_rect(hdc, body, color, px(3, dpi), 1);
    let inner = RECT {
        left: body.left + px(3, dpi),
        top: body.top + px(3, dpi),
        right: body.right - px(3, dpi),
        bottom: body.bottom - px(3, dpi),
    };
    stroke_round_rect(hdc, inner, color, px(2, dpi), 1);
    let mid_y = (rect.top + rect.bottom) / 2;
    stroke_line(hdc, rect.left, mid_y, body.left, mid_y, color, 1);
    stroke_line(hdc, body.right, mid_y, rect.right, mid_y, color, 1);
}

fn draw_memory_icon(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    dpi: i32,
) {
    let gap = px(2, dpi);
    let bar_w = ((rect.right - rect.left) - gap * 2) / 3;
    for index in 0..3 {
        let left = rect.left + index * (bar_w + gap);
        stroke_round_rect(
            hdc,
            RECT {
                left,
                top: rect.top + px(2, dpi),
                right: left + bar_w,
                bottom: rect.bottom - px(2, dpi),
            },
            color,
            px(2, dpi),
            1,
        );
    }
}

fn draw_storage_icon(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    dpi: i32,
) {
    let pad = px(2, dpi);
    let body = RECT {
        left: rect.left + pad,
        top: rect.top + pad + px(2, dpi),
        right: rect.right - pad,
        bottom: rect.bottom - pad,
    };
    stroke_round_rect(hdc, body, color, px(3, dpi), 1);
    let platter = RECT {
        left: body.left + px(3, dpi),
        top: body.top + px(3, dpi),
        right: body.right - px(3, dpi),
        bottom: body.bottom - px(5, dpi),
    };
    stroke_ellipse(hdc, platter, color);
}

fn fill_round_rect(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    radius: i32,
) {
    let brush = unsafe { CreateSolidBrush(color) };
    if brush.is_null() {
        return;
    }
    let previous_brush = unsafe { SelectObject(hdc, brush) };
    let previous_pen = unsafe { SelectObject(hdc, GetStockObject(NULL_PEN)) };
    let _ = unsafe {
        RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius,
            radius,
        )
    };
    if !previous_brush.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_brush) };
    }
    if !previous_pen.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_pen) };
    }
    let _ = unsafe { DeleteObject(brush) };
}

fn stroke_round_rect(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    radius: i32,
    width: i32,
) {
    let pen = unsafe { CreatePen(PS_SOLID, width, color) };
    if pen.is_null() {
        return;
    }
    let previous_pen = unsafe { SelectObject(hdc, pen) };
    let previous_brush = unsafe { SelectObject(hdc, GetStockObject(NULL_BRUSH)) };
    let _ = unsafe {
        RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius,
            radius,
        )
    };
    if !previous_pen.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_pen) };
    }
    if !previous_brush.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_brush) };
    }
    let _ = unsafe { DeleteObject(pen) };
}

fn stroke_ellipse(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: RECT, color: COLORREF) {
    let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
    if pen.is_null() {
        return;
    }
    let previous_pen = unsafe { SelectObject(hdc, pen) };
    let previous_brush = unsafe { SelectObject(hdc, GetStockObject(NULL_BRUSH)) };
    let _ = unsafe { Ellipse(hdc, rect.left, rect.top, rect.right, rect.bottom) };
    if !previous_pen.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_pen) };
    }
    if !previous_brush.is_null() {
        let _ = unsafe { SelectObject(hdc, previous_brush) };
    }
    let _ = unsafe { DeleteObject(pen) };
}

fn stroke_line(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: COLORREF,
    width: i32,
) {
    let pen = unsafe { CreatePen(PS_SOLID, width, color) };
    if pen.is_null() {
        return;
    }
    let previous = unsafe { SelectObject(hdc, pen) };
    let _ = unsafe { MoveToEx(hdc, x1, y1, ptr::null_mut()) };
    let _ = unsafe { LineTo(hdc, x2, y2) };
    if !previous.is_null() {
        let _ = unsafe { SelectObject(hdc, previous) };
    }
    let _ = unsafe { DeleteObject(pen) };
}

#[must_use]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, format_percent};

    #[test]
    fn component_flyout_formatters_cover_unknown_and_scaled_values() {
        assert_eq!(format_percent(None), "--.-%");
        assert_eq!(format_percent(Some(12.34)), "12.3%");
        assert_eq!(format_bytes(1536, 1), "1.5 KB");
        assert_eq!(format_bytes(3_221_225_472, 2), "3.00 GB");
    }
}
