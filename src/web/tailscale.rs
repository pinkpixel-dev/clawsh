//! Tailscale for phone access: finding this machine's tailnet name, and
//! running `tailscale serve` while the server is up so the phone gets HTTPS
//! from anywhere on the tailnet.
//!
//! `serve` runs in the foreground as our child, never with `--bg`. Tailscale
//! drops a foreground config when its process exits, and the child is told
//! to exit when Claw$h does, so a crash can't leave this machine published.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// How long `serve` gets to fail before we call it started.
const STARTUP: Duration = Duration::from_millis(700);

#[derive(Deserialize)]
struct Status {
    #[serde(rename = "BackendState")]
    backend_state: String,
    #[serde(rename = "Self")]
    me: Option<Peer>,
    #[serde(rename = "CertDomains")]
    cert_domains: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Peer {
    #[serde(rename = "DNSName")]
    dns_name: String,
}

/// This machine's `*.ts.net` name, or a short reason it can't be used.
pub fn tailnet_name() -> Result<String, String> {
    let out = Command::new("tailscale")
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "tailscale isn't installed".to_owned())?;
    let status: Status =
        serde_json::from_slice(&out.stdout).map_err(|_| "tailscaled isn't running".to_owned())?;
    name_from(status)
}

fn name_from(status: Status) -> Result<String, String> {
    if status.backend_state != "Running" {
        return Err(format!("Tailscale is {}", status.backend_state.to_lowercase()));
    }
    let name = status.me.map(|p| p.dns_name.trim_end_matches('.').to_owned()).unwrap_or_default();
    if name.is_empty() {
        return Err("this machine has no tailnet name".into());
    }
    // Serve's HTTPS needs a certificate, which only exists with HTTPS
    // Certificates switched on in the admin console.
    if !status.cert_domains.unwrap_or_default().iter().any(|d| d == &name) {
        return Err("HTTPS certificates are off for this tailnet".into());
    }
    Ok(name)
}

/// A running `tailscale serve`. Dropping it stops it.
pub struct Serve {
    child: Child,
}

impl Serve {
    /// Proxies `https://<name>:<port>` on the tailnet to `127.0.0.1:<port>`.
    pub fn start(port: u16) -> Result<Self, String> {
        let mut cmd = Command::new("tailscale");
        cmd.args(["serve", "--yes", &format!("--https={port}"), &format!("http://127.0.0.1:{port}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        // SAFETY: prctl only changes this child's own death signal, and is
        // safe to call between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = cmd.spawn().map_err(|_| "couldn't run tailscale".to_owned())?;

        // Serve fails fast (no operator permission, port taken), so a short
        // wait catches most problems while the reason is still readable.
        let until = Instant::now() + STARTUP;
        while Instant::now() < until {
            if let Ok(Some(_)) = child.try_wait() {
                let mut err = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut err);
                }
                return Err(reason(&err));
            }
            thread::sleep(Duration::from_millis(50));
        }
        Ok(Self { child })
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        // Like pressing ctrl+c on it, then a hard kill if it hangs around.
        // SAFETY: signalling our own child by the pid we spawned.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGINT);
        }
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Serve's errors are a few lines; the first one usually says it all.
fn reason(stderr: &str) -> String {
    let line = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("tailscale serve stopped");
    if line.contains("Access denied") || line.contains("permission") {
        "tailscale serve needs `sudo tailscale set --operator=$USER`".into()
    } else {
        let mut line = line.to_owned();
        line.truncate(80);
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(json: &str) -> Status {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn running_with_certs_gives_the_name() {
        let s = status(r#"{"BackendState":"Running","Self":{"DNSName":"box.tail1.ts.net."},"CertDomains":["box.tail1.ts.net"]}"#);
        assert_eq!(name_from(s), Ok("box.tail1.ts.net".into()));
    }

    #[test]
    fn says_why_it_cant_be_used() {
        let stopped = status(r#"{"BackendState":"Stopped","Self":null,"CertDomains":null}"#);
        assert_eq!(name_from(stopped), Err("Tailscale is stopped".into()));
        let no_certs = status(r#"{"BackendState":"Running","Self":{"DNSName":"box.tail1.ts.net."},"CertDomains":null}"#);
        assert!(name_from(no_certs).unwrap_err().contains("HTTPS certificates"));
    }

    #[test]
    fn permission_errors_point_at_the_fix() {
        assert!(reason("\nAccess denied: serve config denied\n").contains("--operator"));
        assert_eq!(reason("port in use"), "port in use");
    }
}
