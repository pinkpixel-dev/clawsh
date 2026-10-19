//! Claude's 5h and weekly limits, read with the locally stored Claude Code
//! token. Mirrors the Claude probe in quota-core (same endpoint, header, and
//! credentials shape) without pulling in Quota's other providers.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::event::AppEvent;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";

/// How often usage refreshes on its own.
const REFRESH_EVERY: Duration = Duration::from_secs(180);
/// Refresh requests (like a turn finishing) never come closer than this.
const MIN_GAP: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub label: &'static str,
    /// Percent of the window still available, 0 to 100.
    pub left: Option<u8>,
    pub resets_at: Option<DateTime<Utc>>,
}

/// Either the windows or a short reason they couldn't be read.
pub type Report = Result<Vec<Window>, String>;

/// Fetches usage on its own thread and reports it as `AppEvent::Usage`.
pub struct UsagePoller {
    refresh: Sender<()>,
}

impl UsagePoller {
    pub fn start(tx: Sender<AppEvent>) -> Self {
        let (refresh, requests) = mpsc::channel::<()>();
        thread::spawn(move || {
            loop {
                if tx.send(AppEvent::Usage(fetch())).is_err() {
                    return;
                }
                let fetched = Instant::now();
                let mut next = fetched + REFRESH_EVERY;
                loop {
                    let wait = next.saturating_duration_since(Instant::now());
                    match requests.recv_timeout(wait) {
                        // Pull the next fetch in, but not closer than MIN_GAP.
                        Ok(()) => next = next.min((fetched + MIN_GAP).max(Instant::now())),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
        });
        Self { refresh }
    }

    /// Asks for a fresh reading soon.
    pub fn refresh(&self) {
        let _ = self.refresh.send(());
    }
}

fn credentials_path() -> Option<PathBuf> {
    let dir = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".claude"),
    };
    Some(dir.join(".credentials.json"))
}

/// The OAuth access token from Claude Code's credentials file.
fn read_token(raw: &str) -> Result<String, String> {
    let parsed: Value = serde_json::from_str(raw).map_err(|_| "credentials unreadable".to_owned())?;
    parsed
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "not signed in".to_owned())
}

/// The two windows the sidebar shows. Model-specific blocks are ignored.
fn parse_response(raw: &Value) -> Vec<Window> {
    let window = |label, block: Option<&Value>| Window {
        label,
        left: block
            .and_then(|b| b.get("utilization"))
            .and_then(Value::as_f64)
            .filter(|used| used.is_finite())
            .map(|used| (100.0 - used.round()).clamp(0.0, 100.0) as u8),
        resets_at: block
            .and_then(|b| b.get("resets_at"))
            .and_then(Value::as_str)
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc)),
    };
    vec![
        window("5h", raw.get("five_hour")),
        window("wk", raw.get("seven_day")),
    ]
}

/// One blocking read of current usage. Never writes anything to disk.
pub fn fetch() -> Report {
    let path = credentials_path().ok_or("no home folder")?;
    let raw = std::fs::read_to_string(path).map_err(|_| "not signed in".to_owned())?;
    let token = read_token(&raw)?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let body = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", BETA_HEADER)
        .header("User-Agent", concat!("clawsh/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|err| match err {
            ureq::Error::StatusCode(401 | 403) => "token expired, open Claude".to_owned(),
            ureq::Error::StatusCode(429) => "rate limited".to_owned(),
            ureq::Error::StatusCode(code) => format!("usage error {code}"),
            _ => "offline".to_owned(),
        })?
        .body_mut()
        .read_to_string()
        .map_err(|_| "bad response".to_owned())?;
    let json: Value = serde_json::from_str(&body).map_err(|_| "bad response".to_owned())?;
    Ok(parse_response(&json))
}

/// "45m", "3h 20m", "2d 5h": how long until a window resets.
pub fn until(reset: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let mins = (reset - now).num_minutes().max(0);
    let (d, h, m) = (mins / 1440, mins / 60 % 24, mins % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn reads_the_token_from_claude_credentials() {
        let raw = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-x","expiresAt":1,"subscriptionType":"pro"}}"#;
        assert_eq!(read_token(raw).unwrap(), "sk-ant-oat-x");
        assert_eq!(read_token(r#"{"claudeAiOauth":{}}"#).unwrap_err(), "not signed in");
        assert_eq!(read_token("nope").unwrap_err(), "credentials unreadable");
    }

    #[test]
    fn turns_utilization_into_percent_left() {
        let raw = json!({
            "five_hour": {"utilization": 64.6, "resets_at": "2026-10-19T09:00:00.123+00:00"},
            "seven_day": {"utilization": 47.0, "resets_at": null},
            "seven_day_opus": {"utilization": 99.0}
        });
        let w = parse_response(&raw);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].label, w[0].left), ("5h", Some(35)));
        assert_eq!(w[0].resets_at, Some(Utc.with_ymd_and_hms(2026, 10, 19, 9, 0, 0).unwrap() + chrono::Duration::milliseconds(123)));
        assert_eq!((w[1].label, w[1].left, w[1].resets_at), ("wk", Some(53), None));
        assert_eq!(parse_response(&json!({}))[0].left, None);
    }

    #[test]
    fn formats_time_until_reset() {
        let now = Utc.with_ymd_and_hms(2026, 10, 19, 0, 0, 0).unwrap();
        let at = |mins| now + chrono::Duration::minutes(mins);
        assert_eq!(until(at(45), now), "45m");
        assert_eq!(until(at(200), now), "3h 20m");
        assert_eq!(until(at(2 * 1440 + 300), now), "2d 5h");
        assert_eq!(until(at(-5), now), "0m");
    }
}
