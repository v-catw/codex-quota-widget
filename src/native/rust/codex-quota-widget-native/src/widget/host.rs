use std::{path::Path, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::ClientToScreen},
    Storage::Packaging::Appx::GetPackageFamilyName,
    System::Threading::*,
    UI::WindowsAndMessaging::*,
};

pub struct Host {
    pub hwnd: HWND,
    process: HANDLE,
}

impl Drop for Host {
    fn drop(&mut self) { unsafe { CloseHandle(self.process); } }
}

impl Host {
    pub fn find() -> Option<Self> {
        unsafe extern "system" fn candidate(hwnd: HWND, state: LPARAM) -> windows_sys::core::BOOL {
            if let Some(host) = identify(hwnd) {
                unsafe { *(state as *mut Option<Host>) = Some(host); }
                0
            } else { 1 }
        }
        let mut found: Option<Host> = None;
        unsafe { EnumWindows(Some(candidate), (&mut found as *mut Option<Host>) as LPARAM); }
        found
    }

    pub fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.process, 0) == WAIT_OBJECT_0 }
    }

    pub fn area(&self) -> Option<RECT> {
        let mut rect = RECT::default();
        let mut origin = POINT::default();
        let mut cloaked = 0u32;
        unsafe {
            DwmGetWindowAttribute(self.hwnd, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4);
            if IsWindowVisible(self.hwnd) == 0 || IsIconic(self.hwnd) != 0 || cloaked != 0
                || GetClientRect(self.hwnd, &mut rect) == 0 || ClientToScreen(self.hwnd, &mut origin) == 0 {
                return None;
            }
        }
        Some(RECT { left: origin.x, top: origin.y, right: origin.x + rect.right, bottom: origin.y + rect.bottom })
    }
}

fn identify(hwnd: HWND) -> Option<Host> {
    unsafe {
        if IsWindowVisible(hwnd) == 0 { return None; }
        let mut class = [0u16; 256];
        let size = GetClassNameW(hwnd, class.as_mut_ptr(), 256).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..size]);
        if matches!(class.as_str(), "ConsoleWindowClass" | "PseudoConsoleWindow" | "CASCADIA_HOSTING_WINDOW_CLASS") {
            return None;
        }
        if class == "ApplicationFrameWindow" {
            unsafe extern "system" fn child(hwnd: HWND, state: LPARAM) -> windows_sys::core::BOOL {
                if let Some(host) = process_for(hwnd, true) {
                    unsafe { *(state as *mut Option<Host>) = Some(host); }
                    0
                } else { 1 }
            }
            let mut found: Option<Host> = None;
            EnumChildWindows(hwnd, Some(child), (&mut found as *mut Option<Host>) as LPARAM);
            return found.map(|mut host| { host.hwnd = hwnd; host });
        }
        process_for(hwnd, false)
    }
}

fn process_for(hwnd: HWND, packaged_only: bool) -> Option<Host> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid);
        if process == null_mut() { return None; }
        let host = Host { hwnd, process };
        let mut family = [0u16; 256];
        let mut length = family.len() as u32;
        if GetPackageFamilyName(process, &mut length, family.as_mut_ptr()) == 0 {
            let name = String::from_utf16_lossy(&family[..length.saturating_sub(1) as usize]);
            return name.eq_ignore_ascii_case("OpenAI.Codex_2p2nqsd0c76g0").then_some(host);
        }
        if packaged_only { return None; }
        let mut path = [0u16; 1024];
        let mut length = path.len() as u32;
        if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) == 0 { return None; }
        let path = String::from_utf16_lossy(&path[..length as usize]);
        Path::new(&path).file_name().and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("Codex.exe") || name.eq_ignore_ascii_case("OpenAI.Codex.exe"))
            .then_some(host)
    }
}
