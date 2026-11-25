mod account;
mod host;
mod render;
mod usage;

use crate::quota;
use render::{Art, GdiPlus, Surface, wide};
use std::{cell::RefCell, f32::consts::TAU, mem::size_of, ptr::{null, null_mut}, process::ExitCode, sync::mpsc, time::{Duration, Instant}};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Threading::CreateMutexW, LibraryLoader::GetModuleHandleW, Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime}},
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const WIDTH: i32 = 220;
const BODY: i32 = 225;
const BUBBLE: i32 = 90;
const TRAY: u32 = WM_APP + 1;
const EXIT: u32 = 1;
const SHOW: u32 = 2;
const THEME: usize = 3;
const MASCOT: usize = 10;
const RINGS: usize = 11;
const BAR: usize = 12;
const SMALL: usize = 20;
const MEDIUM: usize = 21;
const LARGE: usize = 22;
const SIZE_MENU: usize = 100;

struct Data {
    quota: Result<quota::Snapshot, &'static str>,
    tokens: Option<u64>,
}

fn start_reader() -> std::io::Result<(mpsc::Receiver<Data>, mpsc::Sender<()>)> {
    let (send, receive) = mpsc::sync_channel(1);
    let (stop, stopped) = mpsc::channel();
    std::thread::Builder::new().name("quota-refresh".into()).spawn(move || {
        let mut usage = usage::Usage::default();
        loop {
            let quota = account::read();
            let delay = if matches!(&quota, Err("请求过于频繁")) { 120 } else { 30 };
            if send.send(Data { quota, tokens: usage.today() }).is_err() { break; }
            if !matches!(stopped.recv_timeout(Duration::from_secs(delay)), Err(mpsc::RecvTimeoutError::Timeout)) {
                break;
            }
        }
    })?;
    Ok((receive, stop))
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
struct Press {
    start: POINT,
    anchor: POINT,
    moved: bool,
}

struct Widget {
    art: Art,
    surface: Option<(i32, i32, Surface)>,
    snapshot: quota::Snapshot,
    anchor: POINT,
    origin: POINT,
    panel: RECT,
    hover_started: Option<Instant>,
    started: Instant,
    clicked: Option<Instant>,
    press: Option<Press>,
    bubble: bool,
    left_down: bool,
    visible: bool,
    tray_available: bool,
    demo: bool,
    watch: bool,
    host: Option<host::Host>,
    last_probe: Option<Instant>,
    host_area: Option<RECT>,
    displayed: bool,
    data: Option<mpsc::Receiver<Data>>,
    stop: Option<mpsc::Sender<()>>,
    footer: String,
    status: Option<&'static str>,
    dark: bool,
    mode: usize,
    sizes: [usize; 3],
}

fn settings_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join("CodexQuotaWidgetRust").join("settings.json"))
}

fn load_settings() -> (bool, usize, [usize; 3]) {
    let settings = settings_path().and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .unwrap_or_default();
    let mode = settings["style"].as_u64().filter(|n| (MASCOT as u64..=BAR as u64).contains(n)).unwrap_or(MASCOT as u64) as usize;
    let sizes = std::array::from_fn(|index| {
        settings["sizes"][index].as_u64().filter(|n| (SMALL as u64..=LARGE as u64).contains(n)).unwrap_or(MEDIUM as u64) as usize
    });
    (settings["dark"].as_bool().unwrap_or(false), mode, sizes)
}

fn save_settings(dark: bool, mode: usize, sizes: [usize; 3]) {
    let Some(path) = settings_path() else { return; };
    let Some(dir) = path.parent() else { return; };
    if std::fs::create_dir_all(dir).is_ok() {
        let value = serde_json::json!({"dark": dark, "style": mode, "sizes": sizes});
        let _ = std::fs::write(path, value.to_string());
    }
}

fn metric(window: Option<&quota::Window>) -> (Option<u32>, String) {
    let Some(window) = window else { return (None, "—".into()); };
    let time = window.reset_at.and_then(local_time).unwrap_or_else(|| "—".into());
    (Some(window.remaining), time)
}

fn local_time(seconds: u64) -> Option<String> {
    let ticks = seconds.checked_add(11_644_473_600)?.checked_mul(10_000_000)?;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) == 0 {
            return None;
        }
    }
    Some(format!("{}/{} {:02}:{:02}", local.wMonth, local.wDay, local.wHour, local.wMinute))
}

fn work_area(point: POINT) -> RECT {
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            return info.rcWork;
        }
        let mut area = RECT::default();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut area as *mut RECT).cast(), 0);
        area
    }
}

fn clamp(value: i32, min: i32, max: i32) -> i32 {
    value.clamp(min, max.max(min))
}

impl Widget {
    fn hit(&self, point: POINT) -> bool {
        self.surface.as_ref().is_some_and(|(_, _, surface)|
            surface.hit(point.x - self.origin.x, point.y - self.origin.y))
    }

    fn size(&self) -> usize {
        self.sizes[self.mode - MASCOT]
    }

    fn update_host(&mut self, hwnd: HWND) -> bool {
        if self.demo { return true; }
        if self.host.as_ref().is_some_and(|host| host.exited()) {
            if !self.watch {
                unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0); }
                return false;
            }
            self.host = None;
            self.data = None;
            self.stop = None;
            self.host_area = None;
            self.snapshot = quota::Snapshot { five_hour: None, weekly: None };
            self.status = Some("正在读取额度…");
            self.footer = usage::label(None);
        }
        if self.host.as_ref().is_some_and(|host| unsafe { IsWindow(host.hwnd) } == 0) {
            if let Some(replacement) = host::Host::find() { self.host = Some(replacement); }
        }
        if self.host.is_none() && self.last_probe.is_none_or(|time| time.elapsed() >= Duration::from_millis(700)) {
            self.last_probe = Some(Instant::now());
            self.host = host::Host::find();
        }
        if self.host.is_some() && self.data.is_none() {
            match start_reader() {
                Ok((data, stop)) => { self.data = Some(data); self.stop = Some(stop); }
                Err(_) => self.status = Some("无法启动额度更新"),
            }
        }
        let area = self.host.as_ref().and_then(host::Host::area);
        if let Some(area) = area {
            if let Some(old) = self.host_area {
                if self.press.is_none() {
                    self.anchor.x += area.left - old.left;
                    self.anchor.y += area.top - old.top;
                }
            } else {
                self.anchor = POINT { x: area.right - WIDTH - 16, y: area.bottom - BODY - 16 };
            }
            self.host_area = Some(area);
        }
        if area.is_some() && self.visible {
            let host = self.host.as_ref().unwrap().hwnd;
            unsafe {
                let sibling = GetWindow(host, GW_HWNDPREV);
                if sibling != hwnd {
                    SetWindowPos(hwnd, if sibling.is_null() || GetWindowLongPtrW(sibling, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 { HWND_TOP } else { sibling },
                        0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                }
            }
        }
        area.is_some()
    }

    fn update_data(&mut self) {
        let Some(data) = self.data.as_ref().and_then(|data| data.try_recv().ok()) else { return; };
        self.footer = usage::label(data.tokens);
        match data.quota {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.status = None;
            }
            Err(message) => self.status = Some(message),
        }
    }

    fn panel_contains(&self, point: POINT) -> bool {
        point.x >= self.panel.left && point.x < self.panel.right
            && point.y >= self.panel.top && point.y < self.panel.bottom && self.hit(point)
    }

    fn theme_hit(&self, point: POINT) -> bool {
        self.mode == BAR && self.panel_contains(point)
            && (point.x - self.panel.left) * 260 >= (self.panel.right - self.panel.left) * 218
    }

    fn render(&mut self, hwnd: HWND) -> bool {
        let dpi_window = self.host.as_ref().map_or(hwnd, |host| host.hwnd);
        let dpi = unsafe { GetDpiForWindow(dpi_window) };
        let (panel_dip, body_dip, size_scale) = match self.mode {
            RINGS => (84, 84, match self.size() { SMALL => 86.0, LARGE => 120.0, _ => 103.0 } / 84.0),
            BAR => (260, 48, match self.size() { SMALL => 240.0, LARGE => 360.0, _ => 288.0 } / 260.0),
            _ => (WIDTH, BODY, match self.size() { SMALL => 0.85, LARGE => 1.18, _ => 1.0 }),
        };
        let mut scale = (if dpi == 0 { 96 } else { dpi }) as f32 / 96.0 * size_scale;
        let mut area = work_area(self.anchor);
        if let Some(host) = self.host_area {
            let intersect = |area: RECT| RECT {
                left: area.left.max(host.left), top: area.top.max(host.top),
                right: area.right.min(host.right), bottom: area.bottom.min(host.bottom),
            };
            area = intersect(area);
            if area.right <= area.left || area.bottom <= area.top {
                let center = POINT { x: (host.left + host.right) / 2, y: (host.top + host.bottom) / 2 };
                area = intersect(work_area(center));
            }
        }
        let (available_width, available_height) = (area.right - area.left, area.bottom - area.top);
        if available_width <= 0 || available_height <= 0 { return false; }
        scale = scale.min(available_width as f32 / panel_dip as f32).min(available_height as f32 / body_dip as f32);
        let px = |n: i32| (n as f32 * scale).round().max(1.0) as i32;
        let panel_width = px(panel_dip);
        let body = px(body_dip);
        let show_bubble = self.bubble && available_width >= px(WIDTH) && available_height >= body + px(BUBBLE);
        let bubble = if show_bubble { px(BUBBLE) } else { 0 };
        self.anchor.x = clamp(self.anchor.x, area.left, area.right - panel_width);
        self.anchor.y = clamp(self.anchor.y, area.top, area.bottom - body);
        let above = show_bubble && (self.anchor.y - bubble >= area.top
            || self.anchor.y + body + bubble > area.bottom);
        let y = clamp(self.anchor.y - if above { bubble } else { 0 }, area.top, area.bottom - body - bubble);
        let panel_y = y + if above { bubble } else { 0 };
        let bubble_left = if show_bubble {
            clamp(self.anchor.x + (panel_width - px(WIDTH)) / 2, area.left, area.right - px(WIDTH))
        } else { self.anchor.x };
        self.origin = POINT { x: self.anchor.x.min(bubble_left), y };
        self.panel = RECT { left: self.anchor.x, right: self.anchor.x + panel_width, top: panel_y, bottom: panel_y + body };
        let width = if show_bubble {
            self.panel.right.max(bubble_left + px(WIDTH)) - self.origin.x
        } else { panel_width };
        let height = body + bubble;
        if !self.surface.as_ref().is_some_and(|&(w, h, _)| w == width && h == height) {
            self.surface = Surface::new(width, height).map(|surface| (width, height, surface));
        }
        let Some((_, _, surface)) = self.surface.as_mut() else { return false; };
        let time = self.started.elapsed().as_secs_f32();
        let reaction = self.clicked.map(|click| click.elapsed().as_secs_f32()).unwrap_or(f32::INFINITY);
        let pressed = self.press.is_some();
        let angle = if reaction < 0.96 { 16.0 * (reaction * TAU / 0.24).sin() } else { 6.0 * (time * TAU / 4.0).sin() };
        let float = if pressed { 0.0 } else { 2.5 * (time * TAU / 4.0).sin() };
        let painted = surface.draw(scale, |g| {
            let y = (self.panel.top - self.origin.y) as f32 / scale;
            let five = self.snapshot.five_hour.as_ref().map(|w| w.remaining);
            let week = self.snapshot.weekly.as_ref().map(|w| w.remaining);
            let stale = self.status.is_some() && (five.is_some() || week.is_some());
            g.offset((self.panel.left - self.origin.x) as f32 / scale, y, || {
                match self.mode {
                    RINGS => g.double_ring(five, week, self.dark),
                    BAR => g.horizontal(five, week, self.dark),
                    _ => {
                        self.art.draw(g, 0.0, float, angle, pressed, reaction < 2.5, self.dark);
                        g.caption(five, week, stale, 198.0, self.dark);
                    }
                }
            });
            if show_bubble {
                let left = metric(self.snapshot.five_hour.as_ref());
                let right = metric(self.snapshot.weekly.as_ref());
                g.offset((bubble_left - self.origin.x) as f32 / scale, 0.0, || {
                    g.bubble(if above { 1.0 } else { body_dip as f32 + 9.0 }, above,
                        (left.0, &left.1), (right.0, &right.1), &self.footer, self.dark, self.mode == MASCOT);
                });
            }
        });
        painted && surface.present(hwnd, self.origin)
    }
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p); }
    p
}

fn tray(hwnd: HWND, operation: u32) -> bool {
    let icon = if operation == NIM_ADD { application_icon() } else { null_mut() };
    let mut item = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY,
        hIcon: if icon.is_null() { unsafe { LoadIconW(null_mut(), IDI_APPLICATION) } } else { icon },
        ..Default::default()
    };
    let tip = wide("codex用量监测 · Rust 0.2.9");
    item.szTip[..tip.len()].copy_from_slice(&tip);
    unsafe {
        let result = Shell_NotifyIconW(operation, &item) != 0;
        if !icon.is_null() { DestroyIcon(icon); }
        result
    }
}

fn application_icon() -> HICON {
    let ico = include_bytes!("../assets/codex-quota-logo.ico");
    let Some(count) = ico.get(4..6).map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]])) else {
        return null_mut();
    };
    let entry = ico[6..].chunks_exact(16).take(count as usize)
        .min_by_key(|entry| if entry[0] == 0 { 224 } else { i32::from(entry[0]).abs_diff(32) });
    let Some(entry) = entry else { return null_mut(); };
    let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
    let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
    let Some(bits) = ico.get(offset..offset.saturating_add(length)) else { return null_mut(); };
    unsafe { CreateIconFromResourceEx(bits.as_ptr(), bits.len() as u32, 1, 0x0003_0000, 0, 0, LR_DEFAULTCOLOR) }
}

fn menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() { return; }
        let can_hide = WIDGET.with(|slot| slot.borrow().as_ref().is_some_and(|widget| widget.tray_available));
        if can_hide {
            AppendMenuW(menu, MF_STRING, SHOW as usize, wide("显示 / 隐藏").as_ptr());
        }
        AppendMenuW(menu, MF_STRING, EXIT as usize, wide("退出 Rust 原型").as_ptr());
        WIDGET.with(|slot| {
            let slot = slot.borrow();
            let Some(widget) = slot.as_ref() else { return; };
            let status = if !widget.demo && widget.host.is_none() {
                Some("等待 Codex 窗口")
            } else {
                widget.status
            };
            if let Some(status) = status {
                AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(status).as_ptr());
            }
            for (id, label, selected) in [
                (THEME, if widget.dark { "切换日间主题" } else { "切换夜间主题" }, false),
                (MASCOT, "样式 · GPT 娘", widget.mode == MASCOT),
                (RINGS, "样式 · 双环", widget.mode == RINGS),
                (BAR, "样式 · 横向", widget.mode == BAR),
            ] {
                AppendMenuW(menu, MF_STRING | if selected { MF_CHECKED } else { 0 }, id, wide(label).as_ptr());
            }
            let sizes = CreatePopupMenu();
            if sizes.is_null() { return; }
            for (style, name) in ["GPT 娘", "双环", "横向"].into_iter().enumerate() {
                let group = CreatePopupMenu();
                if group.is_null() { continue; }
                for (index, label) in ["小", "中", "大"].into_iter().enumerate() {
                    let check = if widget.sizes[style] == SMALL + index { MF_CHECKED } else { 0 };
                    AppendMenuW(group, MF_STRING | check, SIZE_MENU + style * 3 + index, wide(label).as_ptr());
                }
                if AppendMenuW(sizes, MF_POPUP, group as usize, wide(name).as_ptr()) == 0 { DestroyMenu(group); }
            }
            if AppendMenuW(menu, MF_POPUP, sizes as usize, wide("悬浮窗尺寸").as_ptr()) == 0 { DestroyMenu(sizes); }
        });
        let point = cursor();
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, null());
        DestroyMenu(menu);
        PostMessageW(hwnd, WM_NULL, 0, 0);
        if command != 0 { PostMessageW(hwnd, WM_COMMAND, command as usize, 0); }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_DESTROY => {
            tray(hwnd, NIM_DELETE);
            unsafe { PostQuitMessage(0); }
            return 0;
        }
        WM_CLOSE => { unsafe { DestroyWindow(hwnd); } return 0; }
        WM_COMMAND if w == EXIT as usize => { unsafe { DestroyWindow(hwnd); } return 0; }
        TRAY if matches!(l as u32, WM_LBUTTONUP | WM_RBUTTONUP) => { menu(hwnd); return 0; }
        WM_RBUTTONUP => { menu(hwnd); return 0; }
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        _ => {}
    }

    let result = WIDGET.with(|slot| {
        let mut slot = slot.try_borrow_mut().ok()?;
        let widget = slot.as_mut()?;
        match message {
            WM_TIMER if w == 2 => {
                widget.update_data();
                let available = widget.update_host(hwnd);
                let show = available && widget.visible;
                if show != widget.displayed {
                    widget.displayed = show;
                    if !show {
                        widget.press = None;
                        widget.bubble = false;
                        unsafe { KillTimer(hwnd, 1); }
                    } else {
                        widget.render(hwnd);
                        unsafe { SetTimer(hwnd, 1, 16, None); }
                    }
                    unsafe { ShowWindow(hwnd, if show { SW_SHOWNOACTIVATE } else { SW_HIDE }); }
                }
            }
            WM_TIMER if w == 1 && widget.displayed => {

                let down = unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } < 0;
                if widget.bubble && down && !widget.left_down && widget.press.is_none() && !widget.hit(cursor()) {
                    widget.bubble = false;
                }
                if widget.mode != MASCOT {
                    let pointer = cursor();
                    if widget.press.is_none() && widget.panel_contains(pointer) && !widget.theme_hit(pointer) {
                        let since = widget.hover_started.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_millis(260) { widget.bubble = true; }
                    } else {
                        widget.hover_started = None;
                        if !widget.hit(pointer) || widget.theme_hit(pointer) { widget.bubble = false; }
                    }
                }
                widget.left_down = down;
                widget.render(hwnd);
            }
            WM_LBUTTONDOWN => {
                let point = cursor();
                if !widget.panel_contains(point) { return Some(0); }
                widget.press = Some(Press { start: point, anchor: widget.anchor, moved: false });
                unsafe { SetCapture(hwnd); }
            }
            WM_MOUSEMOVE => {
                if let Some(press) = &mut widget.press {
                    let point = cursor();
                    let (dx, dy) = (point.x - press.start.x, point.y - press.start.y);
                    press.moved |= dx.abs() >= 4 || dy.abs() >= 4;
                    if press.moved {
                        widget.bubble = false;
                        widget.anchor = POINT { x: press.anchor.x + dx, y: press.anchor.y + dy };
                    }
                }
            }
            WM_LBUTTONUP => {
                if let Some(press) = widget.press.take() {
                    unsafe { ReleaseCapture(); }
                    if !press.moved && widget.theme_hit(press.start) && widget.theme_hit(cursor()) {
                        widget.dark = !widget.dark;
                        widget.bubble = false;
                        if !widget.demo { save_settings(widget.dark, widget.mode, widget.sizes); }
                    } else if !press.moved && widget.mode == MASCOT && widget.panel_contains(cursor()) {
                        widget.bubble = !widget.bubble;
                        widget.clicked = Some(Instant::now());
                    }
                    widget.render(hwnd);
                }
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => { widget.press = None; }
            WM_COMMAND if w == SHOW as usize => {
                widget.visible = !widget.visible;
                widget.bubble = false;

            }
            WM_COMMAND if matches!(w, THEME | MASCOT | RINGS | BAR) || (SIZE_MENU..SIZE_MENU + 9).contains(&w) => {
                match w {
                    THEME => widget.dark = !widget.dark,
                    MASCOT | RINGS | BAR => widget.mode = w,
                    _ => {
                        let selected = w - SIZE_MENU;
                        widget.sizes[selected / 3] = SMALL + selected % 3;
                    }
                }
                widget.bubble = false;
                widget.hover_started = None;
                if !widget.demo { save_settings(widget.dark, widget.mode, widget.sizes); }
                if widget.displayed { widget.render(hwnd); }
            }
            _ => return None,
        }
        Some(0)
    });

    result.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, message, w, l) })
}

pub fn run() -> ExitCode {
    let _gdiplus = match GdiPlus::start() {
        Some(runtime) => runtime,
        None => return fail("无法初始化 Windows 绘图组件。"),
    };
    if let Err(message) = launch() { return fail(message); }
    ExitCode::SUCCESS
}

fn fail(message: &str) -> ExitCode {
    unsafe { MessageBoxW(null_mut(), wide(message).as_ptr(), wide("Rust 原型").as_ptr(), MB_ICONERROR); }
    ExitCode::FAILURE
}

fn launch() -> Result<(), &'static str> {
    let demo = std::env::args_os().any(|arg| arg == "--demo");
    let watch = std::env::args_os().any(|arg| arg == "--watch");
    let snapshot = if demo { quota::parse(quota::DEMO).map_err(|_| "模拟额度格式无效。")? }
        else { quota::Snapshot { five_hour: None, weekly: None } };
    let _single_instance = if demo { None } else {
        struct Instance(HANDLE);
        impl Drop for Instance {
            fn drop(&mut self) { unsafe { CloseHandle(self.0); } }
        }
        let (handle, previous) = unsafe {
            let handle = CreateMutexW(null(), 0, wide("Local\\CodexQuotaWidgetRust").as_ptr());
            (handle, GetLastError())
        };
        if handle.is_null() { return Err("无法创建单实例锁。"); }
        let instance = Instance(handle);
        if previous == ERROR_ALREADY_EXISTS { return Ok(()); }
        Some(instance)
    };
    let art = Art::load().ok_or("无法读取内嵌人物素材。")?;
    let (dark, mode, sizes) = if demo { (false, MASCOT, [MEDIUM; 3]) } else { load_settings() };
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(null());
        let class_name = wide("CodexQuotaWidgetRustPrototype");
        let class = WNDCLASSW {
            lpszClassName: class_name.as_ptr(),
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_HAND),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 { return Err("无法注册窗口类。"); }
        let area = work_area(cursor());
        let anchor = POINT { x: (area.right - WIDTH - 24).max(area.left), y: (area.bottom - BODY - 24).max(area.top) };
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | if demo { WS_EX_TOPMOST } else { 0 },
            class_name.as_ptr(), wide("codex用量监测 · Rust 0.2.9").as_ptr(), WS_POPUP,
            anchor.x, anchor.y, WIDTH, BODY, null_mut(), null_mut(), instance, null(),
        );
        if hwnd.is_null() { return Err("无法创建悬浮窗。"); }
        let mut widget = Widget {
            art, snapshot, surface: None, origin: anchor, panel: RECT::default(), hover_started: None, anchor, started: Instant::now(),
            clicked: None, press: None, bubble: false, left_down: false, visible: true, tray_available: false,
            demo, watch, host: None, last_probe: None, host_area: None, displayed: false,
            data: None, stop: None,
            footer: if demo { "今日用量（演示） 124k token".into() } else { usage::label(None) },
            status: if demo { None } else { Some("正在读取额度…") },
            dark, mode, sizes,
        };
        if !widget.render(hwnd) {
            DestroyWindow(hwnd);
            return Err("无法绘制透明悬浮窗。");
        }
        WIDGET.with(|slot| slot.replace(Some(widget)));
        if SetTimer(hwnd, 2, 250, None) == 0 {
            WIDGET.with(|slot| slot.replace(None));
            DestroyWindow(hwnd);
            return Err("无法启动窗口跟随定时器。");
        }
        // The window's own menu remains useful when the shell tray is unavailable.
        let tray_available = tray(hwnd, NIM_ADD);
        WIDGET.with(|slot| {
            if let Some(widget) = slot.borrow_mut().as_mut() {
                widget.tray_available = tray_available;
            }
        });

        let mut msg = MSG::default();
        let status = loop {
            let status = GetMessageW(&mut msg, null_mut(), 0, 0);
            if status <= 0 { break status; }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        };
        if IsWindow(hwnd) != 0 { DestroyWindow(hwnd); }
        WIDGET.with(|slot| slot.replace(None));
        if status < 0 { return Err("Windows 消息循环异常退出。"); }
    }
    Ok(())
}
