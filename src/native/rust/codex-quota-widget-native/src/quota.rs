use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct Window {
    pub remaining: u32,
    pub reset_at: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub struct Snapshot {
    pub five_hour: Option<Window>,
    pub weekly: Option<Window>,
}

pub fn parse(json: &str) -> Result<Snapshot, serde_json::Error> {
    let root: Value = serde_json::from_str(json)?;
    let limits = &root["rate_limit"];
    Ok(Snapshot {
        five_hour: window(&limits["primary_window"]),
        weekly: window(&limits["secondary_window"]),
    })
}

fn window(value: &Value) -> Option<Window> {
    let used = &value["used_percent"];
    let percent = used.as_f64().or_else(|| used.as_str()?.parse().ok())?;
    if !percent.is_finite() {
        return None;
    }
    let reset = &value["reset_at"];
    let reset_at = reset.as_u64().or_else(|| reset.as_str()?.parse().ok());
    Some(Window {
        remaining: (100.0 - percent.clamp(0.0, 100.0)).round_ties_even() as u32,
        reset_at,
    })
}

pub const DEMO: &str = include_str!("../../../../../testdata/usage-success.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_shared_quota_fixtures() {
        let demo = parse(DEMO).unwrap();
        assert_eq!(demo.five_hour.unwrap(), Window { remaining: 75, reset_at: Some(1790208000) });
        assert_eq!(demo.weekly.unwrap(), Window { remaining: 40, reset_at: Some(1790380800) });
        let empty = parse(include_str!("../../../../../testdata/usage-empty-window.json")).unwrap();
        assert_eq!(empty.five_hour, None);
        assert_eq!(empty.weekly, None);
        assert!(parse(include_str!("../../../../../testdata/usage-invalid.json")).is_err());
    }
}
