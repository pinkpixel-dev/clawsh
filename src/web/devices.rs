//! Paired phones and the pairing code that adds them.
//!
//! Pairing trades a short one-time code (shown at the desk as a QR code and
//! as text) for a long random device token, which the phone keeps in an
//! `HttpOnly` cookie. Only a SHA-256 of each token is saved. Tokens are 128
//! random bits, so a plain hash is enough; there's nothing to brute-force.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How long a pairing code works.
pub const CODE_LIFETIME: Duration = Duration::from_secs(5 * 60);
/// Wrong guesses before the code is thrown away.
const MAX_ATTEMPTS: u32 = 5;
/// No 0/O or 1/I/L, so a typed code can't be misread.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
const CODE_LEN: usize = 8;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Can see the screen but anything it sends is dropped.
    pub watch_only: bool,
    pub paired: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    token_hash: String,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    devices: Vec<Device>,
}

/// Every paired device, saved to `path` when there is one. Test runs with
/// another program keep them in memory only, like the session.
pub struct Devices {
    path: Option<PathBuf>,
    list: Vec<Device>,
}

impl Devices {
    pub fn load(path: Option<PathBuf>) -> Self {
        let list = path
            .as_deref()
            .and_then(|p| fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
            .map(|f| f.devices)
            .unwrap_or_default();
        Self { path, list }
    }

    pub fn list(&self) -> &[Device] {
        &self.list
    }

    /// Adds a device and returns its token, which only the phone keeps.
    pub fn add(&mut self, name: String, watch_only: bool) -> io::Result<(Device, String)> {
        let token = random_hex(16)?;
        let device = Device {
            id: random_hex(4)?,
            name,
            watch_only,
            paired: Utc::now(),
            last_seen: None,
            token_hash: hash(&token),
        };
        self.list.push(device.clone());
        self.save()?;
        Ok((device, token))
    }

    pub fn by_token(&self, token: &str) -> Option<&Device> {
        let h = hash(token);
        self.list.iter().find(|d| d.token_hash == h)
    }

    pub fn touch(&mut self, id: &str) {
        if let Some(d) = self.list.iter_mut().find(|d| d.id == id) {
            d.last_seen = Some(Utc::now());
            let _ = self.save();
        }
    }

    pub fn remove(&mut self, id: &str) -> io::Result<()> {
        self.list.retain(|d| d.id != id);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        save_atomic(path, &File { devices: self.list.clone() })
    }
}

fn save_atomic(path: &Path, file: &File) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(file)?)?;
    // Tokens can't be recovered from the file, but it's still nobody else's.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(tmp, path)
}

/// The code currently shown at the desk.
pub struct Pairing {
    code: String,
    pub watch_only: bool,
    expires: Instant,
    misses: u32,
}

impl Pairing {
    pub fn new(watch_only: bool) -> io::Result<Self> {
        let mut bytes = [0u8; CODE_LEN];
        urandom(&mut bytes)?;
        // 256 isn't a multiple of 30, so a few letters come up slightly more
        // often. At ~39 bits with five tries, that doesn't matter.
        let code = bytes.iter().map(|b| CODE_ALPHABET[*b as usize % CODE_ALPHABET.len()] as char).collect();
        Ok(Self { code, watch_only, expires: Instant::now() + CODE_LIFETIME, misses: 0 })
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// `K7QM-2XPV`, easier to read off a screen.
    pub fn display(&self) -> String {
        format!("{}-{}", &self.code[..4], &self.code[4..])
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.expires || self.misses >= MAX_ATTEMPTS
    }

    pub fn remaining(&self) -> Duration {
        self.expires.saturating_duration_since(Instant::now())
    }

    /// Checks a typed or scanned code. Spaces, dashes, and case don't matter.
    /// A wrong guess counts toward the limit.
    pub fn check(&mut self, attempt: &str) -> bool {
        if self.expired() {
            return false;
        }
        let attempt: String =
            attempt.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
        let ok = attempt.len() == self.code.len()
            && attempt.bytes().zip(self.code.bytes()).fold(0, |acc, (a, b)| acc | (a ^ b)) == 0;
        if !ok {
            self.misses += 1;
        }
        ok
    }
}

/// A short name for the device list, guessed from the browser.
pub fn name_from_agent(agent: &str) -> String {
    let kind = ["iPhone", "iPad", "Android", "Macintosh", "Windows", "Linux"]
        .into_iter()
        .find(|k| agent.contains(k))
        .map(|k| if k == "Macintosh" { "Mac" } else { k })
        .unwrap_or("Browser");
    format!("{kind} · {}", Utc::now().format("%b %-d"))
}

fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn random_hex(len: usize) -> io::Result<String> {
    let mut bytes = vec![0u8; len];
    urandom(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn urandom(buf: &mut [u8]) -> io::Result<()> {
    fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clawsh-devices-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("devices.json")
    }

    #[test]
    fn tokens_are_only_kept_as_hashes() {
        let path = temp_file("hash");
        let mut devices = Devices::load(Some(path.clone()));
        let (device, token) = devices.add("iPhone · Oct 20".into(), false).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains(&token), "raw token must not be saved");
        assert!(saved.contains(&hash(&token)));
        assert_eq!(devices.by_token(&token).map(|d| &d.id), Some(&device.id));
        assert!(devices.by_token("nope").is_none());
    }

    #[test]
    fn devices_survive_a_reload_and_can_be_removed() {
        let path = temp_file("reload");
        let mut devices = Devices::load(Some(path.clone()));
        let (device, token) = devices.add("Mac".into(), true).unwrap();
        let mut again = Devices::load(Some(path.clone()));
        assert!(again.by_token(&token).is_some_and(|d| d.watch_only));
        again.remove(&device.id).unwrap();
        assert!(Devices::load(Some(path)).by_token(&token).is_none());
    }

    #[test]
    fn codes_ignore_case_and_dashes() {
        let mut p = Pairing::new(false).unwrap();
        assert_eq!(p.code().len(), 8);
        assert!(p.code().bytes().all(|b| CODE_ALPHABET.contains(&b)));
        let typed = p.display().to_lowercase();
        assert!(p.check(&typed));
    }

    #[test]
    fn five_wrong_guesses_burn_the_code() {
        let mut p = Pairing::new(false).unwrap();
        let right = p.code().to_owned();
        for _ in 0..MAX_ATTEMPTS {
            assert!(!p.check("AAAAAAAA0"));
        }
        assert!(p.expired());
        assert!(!p.check(&right), "even the right code fails now");
    }

    #[test]
    fn names_come_from_the_browser() {
        assert!(name_from_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X)").starts_with("iPhone · "));
        assert!(name_from_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)").starts_with("Mac · "));
        assert!(name_from_agent("curl/8").starts_with("Browser · "));
    }
}
