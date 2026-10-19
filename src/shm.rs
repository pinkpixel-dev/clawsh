//! Hands image pixels to the host terminal through POSIX shared memory.
//!
//! Instead of base64 over the terminal, each frame is written to a new object
//! in `/dev/shm` (which is what `shm_open` uses on Linux) and the host gets a
//! short `t=s` command naming it. kitty and Ghostty read the object and unlink
//! it themselves. Anything a terminal leaves behind is deleted after
//! [`LEFTOVER_TTL`], and everything is deleted on exit, so a terminal that never
//! reads them can't fill up memory.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long an object may sit unread before we delete it ourselves.
const LEFTOVER_TTL: Duration = Duration::from_secs(2);

pub struct ShmWriter {
    dir: PathBuf,
    prefix: String,
    next: u64,
    written: VecDeque<(Instant, PathBuf)>,
}

impl ShmWriter {
    /// Shared memory only works when the terminal runs on this machine, so
    /// it's off over SSH, when `/dev/shm` is missing, or when
    /// `CLAWSH_IMAGES=direct` asks for plain base64.
    pub fn detect() -> Option<Self> {
        let forced_direct = std::env::var("CLAWSH_IMAGES").is_ok_and(|v| v == "direct");
        let over_ssh = std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
        let dir = PathBuf::from("/dev/shm");
        (!forced_direct && !over_ssh && dir.is_dir()).then(|| Self::in_dir(dir))
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            dir,
            prefix: format!("clawsh-{}-", std::process::id()),
            next: 0,
            written: VecDeque::new(),
        }
    }

    /// Writes `bytes` to a fresh object and returns the name to send the
    /// terminal (POSIX style, with a leading slash).
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<String> {
        let name = format!("{}{}", self.prefix, self.next);
        self.next += 1;
        let path = self.dir.join(&name);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?
            .write_all(bytes)?;
        self.written.push_back((Instant::now(), path));
        Ok(format!("/{name}"))
    }

    /// Deletes objects the terminal should have read by now. Normally they're
    /// already gone, and the failed remove is harmless.
    pub fn expire(&mut self) {
        while let Some((at, _)) = self.written.front() {
            if at.elapsed() < LEFTOVER_TTL {
                break;
            }
            if let Some((_, path)) = self.written.pop_front() {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// Deletes everything still around, for exit.
    pub fn cleanup(&mut self) {
        for (_, path) in self.written.drain(..) {
            let _ = fs::remove_file(path);
        }
    }
}

impl Drop for ShmWriter {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clawsh-shm-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_private_objects_with_unique_names() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("write");
        let mut shm = ShmWriter::in_dir(dir.clone());
        let a = shm.write(b"abc").unwrap();
        let b = shm.write(b"de").unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("/clawsh-"));
        let path = dir.join(a.trim_start_matches('/'));
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn leftovers_go_away() {
        let dir = temp("cleanup");
        let mut shm = ShmWriter::in_dir(dir.clone());
        let name = shm.write(b"x").unwrap();
        let path = dir.join(name.trim_start_matches('/'));
        shm.expire();
        assert!(path.exists(), "too new to expire");
        drop(shm);
        assert!(!path.exists(), "deleted on exit");
        fs::remove_dir_all(dir).unwrap();
    }
}
