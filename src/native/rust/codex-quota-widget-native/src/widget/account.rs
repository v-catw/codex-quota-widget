use super::render::wide;
use crate::quota::{self, Snapshot};
use serde_json::Value;
use std::{ffi::c_void, fs::File, io::Read, path::PathBuf, ptr::{null, null_mut}};
use windows_sys::Win32::Networking::WinHttp::*;

pub fn home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME").filter(|path| !path.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|path| PathBuf::from(path).join(".codex")))
}

struct Http(*mut c_void);
impl Http {
    fn open(ptr: *mut c_void) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr))
    }
}
impl Drop for Http {
    fn drop(&mut self) { unsafe { WinHttpCloseHandle(self.0); } }
}

pub fn read() -> Result<Snapshot, &'static str> {
    let file = File::open(home().ok_or("未找到 Codex 目录")?.join("auth.json"))
        .map_err(|_| "请先登录 Codex")?;
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "无法读取 Codex 登录状态")?;
    if bytes.len() > 256 * 1024 { return Err("Codex 登录文件过大"); }
    let auth: Value = serde_json::from_slice(&bytes).map_err(|_| "Codex 登录格式无效")?;
    let field = |name: &str| auth["tokens"][name].as_str().or_else(|| auth[name].as_str());
    let valid = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic());
    let token = field("access_token").filter(|v| valid(v)).ok_or("请先登录 Codex")?;
    let mut headers = format!("Authorization: Bearer {token}\r\nAccept: application/json\r\nOAI-Product-Sku: codex\r\nOriginator: codex_cli_rs\r\n");
    if let Some(account) = field("account_id").filter(|v| valid(v)) {
        headers.push_str(&format!("ChatGPT-Account-Id: {account}\r\n"));
    }
    let response = request(&wide(&headers))?;
    let json = std::str::from_utf8(&response).map_err(|_| "额度响应无法解析")?;
    let snapshot = quota::parse(json).map_err(|_| "额度响应无法解析")?;
    if snapshot.five_hour.is_none() && snapshot.weekly.is_none() {
        return Err("账户暂无可用额度");
    }
    Ok(snapshot)
}

fn request(headers: &[u16]) -> Result<Vec<u8>, &'static str> {
    let unavailable = "额度查询暂不可用";
    unsafe {
        let session = Http::open(WinHttpOpen(wide("codex-cli").as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(), null(), 0)).ok_or(unavailable)?;
        if WinHttpSetTimeouts(session.0, 3000, 5000, 5000, 5000) == 0 { return Err(unavailable); }
        let connection = Http::open(WinHttpConnect(session.0, wide("chatgpt.com").as_ptr(), 443, 0)).ok_or(unavailable)?;
        let request = Http::open(WinHttpOpenRequest(connection.0, wide("GET").as_ptr(),
            wide("/backend-api/wham/usage").as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE)).ok_or(unavailable)?;
        // Never forward the Authorization header to a redirect destination.
        let policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        if WinHttpSetOption(request.0, WINHTTP_OPTION_REDIRECT_POLICY, (&policy as *const u32).cast(), 4) == 0
            || WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX, null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0 { return Err(unavailable); }
        let mut code = 0u32;
        let mut size = 4;
        if WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(), (&mut code as *mut u32).cast(), &mut size, null_mut()) == 0 { return Err(unavailable); }
        match code {
            200 => {},
            401 | 403 => return Err("请检查 Codex 登录"),
            429 => return Err("请求过于频繁"),
            _ => return Err(unavailable),
        }
        let started = std::time::Instant::now();
        let mut buffer = [0u8; 8192];
        let mut response = Vec::new();
        loop {
            let mut read = 0;
            if started.elapsed().as_secs() >= 10
                || WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut read) == 0 {
                return Err(unavailable);
            }
            if read == 0 { return Ok(response); }
            response.extend_from_slice(&buffer[..read as usize]);
            if response.len() > 1024 * 1024 { return Err("额度响应过大"); }
        }
    }
}
