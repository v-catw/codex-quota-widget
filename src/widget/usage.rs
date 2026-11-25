use super::account;
use serde_json::Value;
use std::{collections::HashMap, fs::{self, File}, io::{BufRead, BufReader}, path::PathBuf, ptr::null, time::SystemTime};
use windows_sys::Win32::{Foundation::{FILETIME, SYSTEMTIME}, System::{SystemInformation::GetLocalTime, Time::*}};

type Cached = (u64, SystemTime, Option<u64>);

#[derive(Default)]
pub struct Usage {
    cache: HashMap<PathBuf, Cached>,
    day: u64,
}

impl Usage {
    pub fn today(&mut self) -> Option<u64> {
        let (start, end) = today_bounds()?;
        if self.day != start { self.cache.clear(); self.day = start; }
        let root = account::home()?;
        let mut files = HashMap::new();
        for dir in [root.join("sessions"), root.join("archived_sessions")] {
            let mut pending = vec![dir];
            while let Some(dir) = pending.pop() {
                let Ok(entries) = fs::read_dir(dir) else { continue; };
                for entry in entries.flatten() {
                    let Ok(kind) = entry.file_type() else { continue; };
                    if kind.is_dir() {
                        use std::os::windows::fs::MetadataExt;
                        if entry.metadata().is_ok_and(|m| m.file_attributes() & 0x400 == 0) {
                            pending.push(entry.path());
                        }
                    } else if kind.is_file() {
                        let name = entry.file_name();
                        let label = name.to_string_lossy();
                        if !label.starts_with("rollout-") || !label.ends_with(".jsonl") { continue; }
                        let Ok(meta) = entry.metadata() else { continue; };
                        let Ok(modified) = meta.modified() else { continue; };
                        let epoch = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
                        if epoch < start { continue; }
                        let current = files.entry(name).or_insert((entry.path(), meta.len(), modified));
                        if modified > current.2 { *current = (entry.path(), meta.len(), modified); }
                    }
                }
            }
        }
        let mut cache = HashMap::new();
        let mut total: Option<u64> = None;
        for (path, length, modified) in files.into_values() {
            let value = match self.cache.get(&path) {
                Some(&(len, time, value)) if len == length && time == modified => value,
                _ => File::open(&path).ok().and_then(|file| sum(BufReader::new(file), start, end)),
            };
            cache.insert(path, (length, modified, value));
            if let Some(value) = value {
                total = Some(total.unwrap_or_default().checked_add(value)?);
            }
        }
        self.cache = cache;
        total
    }
}

fn ticks(time: &SYSTEMTIME) -> Option<u64> {
    let mut file = FILETIME::default();
    if unsafe { SystemTimeToFileTime(time, &mut file) } == 0 { return None; }
    Some((u64::from(file.dwHighDateTime) << 32) | u64::from(file.dwLowDateTime))
}

fn epoch(time: &SYSTEMTIME) -> Option<u64> {
    Some((ticks(time)? / 10_000_000).checked_sub(11_644_473_600)?)
}

fn today_bounds() -> Option<(u64, u64)> {
    let mut start = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut start); }
    start.wHour = 0;
    start.wMinute = 0;
    start.wSecond = 0;
    start.wMilliseconds = 0;
    let next = ticks(&start)? + 864_000_000_000;
    let next = FILETIME { dwLowDateTime: next as u32, dwHighDateTime: (next >> 32) as u32 };
    let mut end = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&next, &mut end) } == 0 { return None; }
    let utc = |local: SYSTEMTIME| {
        let mut result = SYSTEMTIME::default();
        if unsafe { TzSpecificLocalTimeToSystemTime(null(), &local, &mut result) } == 0 { return None; }
        epoch(&result)
    };
    Some((utc(start)?, utc(end)?))
}

fn timestamp(s: &str) -> Option<u64> {
    let number = |start, end| s.get(start..end)?.parse().ok();
    let date = SYSTEMTIME {
        wYear: number(0, 4)?, wMonth: number(5, 7)?, wDay: number(8, 10)?,
        wHour: number(11, 13)?, wMinute: number(14, 16)?, wSecond: number(17, 19)?,
        ..Default::default()
    };
    let base = epoch(&date)?;
    let tail = s.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if tail == "Z" { return Some(base); }
    let hour = tail.get(1..3)?.parse::<u64>().ok()?;
    let min = tail.get(4..6)?.parse::<u64>().ok()?;
    if tail.len() != 6 || hour > 23 || min > 59 || tail.as_bytes()[3] != b':' { return None; }
    let offset = hour * 3600 + min * 60;
    match tail.as_bytes()[0] { b'+' => base.checked_sub(offset), b'-' => base.checked_add(offset), _ => None }
}

fn sum(reader: impl BufRead, start: u64, end: u64) -> Option<u64> {
    let mut previous: Option<u64> = None;
    let mut total = 0u64;
    for line in reader.lines() {
        let line = line.ok()?;
        if !line.contains("\"token_count\"") { continue; }
        let Ok(data) = serde_json::from_str::<Value>(&line) else { continue; };
        let info = &data["payload"]["info"];
        if data["type"] != "event_msg" || data["payload"]["type"] != "token_count" { continue; }
        let Some(time) = data["timestamp"].as_str().and_then(timestamp) else { continue; };
        let Some(current) = info["total_token_usage"]["total_tokens"].as_u64() else { continue; };
        let delta = match previous {
            Some(previous) => current.saturating_sub(previous),
            None => info["last_token_usage"]["total_tokens"].as_u64().map_or(current, |value| value.min(current)),
        };
        previous = Some(previous.unwrap_or_default().max(current));
        if (start..end).contains(&time) { total = total.checked_add(delta)?; }
    }
    previous.map(|_| total)
}

pub fn label(tokens: Option<u64>) -> String {
    let Some(tokens) = tokens else { return "今日用量（本机） —".into(); };
    let mut amount = tokens as f64;
    let mut unit = "";
    for next in ["k", "M", "B", "T", "P", "E"] {
        if amount < 999.95 { break; }
        amount /= 1000.0;
        unit = next;
    }
    let suffix = format!("{amount:.1}");
    format!("今日用量（本机） {}{unit} token", suffix.trim_end_matches(".0"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn counts_only_in_window_without_duplicating_snapshots() {
        let event = |t: &str, n: u32| serde_json::json!({
            "type": "event_msg", "timestamp": t, "payload": {"type":"token_count","info":{
                "total_token_usage":{"total_tokens":n}, "last_token_usage":{"total_tokens":10}
            }}
        }).to_string();
        let lines = [
            event("2026-11-23T23:59:59Z", 100),
            event("2026-11-24T08:00:01.25+08:00", 120),
            event("2026-11-24T00:00:02Z", 120),
            event("2026-11-25T00:00:00Z", 150),
        ].join("\n");
        let start = timestamp("2026-11-24T00:00:00Z").unwrap();
        assert_eq!(sum(Cursor::new(lines), start, start + 86400), Some(20));
        assert_eq!(label(Some(999_999)), "今日用量（本机） 1M token");
    }
}
