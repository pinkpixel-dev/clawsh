//! Starts each pane's program in its own systemd scope.
//!
//! Without this every tab shares the host terminal's cgroup, so when
//! systemd-oomd steps in it kills the whole window: Claw$h and every
//! workspace with it. In a scope of its own, a tab that runs away with memory
//! is the biggest cgroup and gets killed alone.
//!
//! `systemd-run --scope` registers the scope and then execs the program in
//! the same process, so the PTY child is still the program itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Wraps `program` in `systemd-run` when scopes work here. Otherwise, or when
/// `program` can't be found (so a typo still fails at spawn instead of
/// becoming a tab that exits the moment it starts), it's returned as is.
pub fn wrap(id: usize, program: &str, args: &[String]) -> (String, Vec<String>) {
    match find_program(program, std::env::var_os("PATH").as_deref()) {
        Some(path) if available() => ("systemd-run".into(), scope_args(std::process::id(), id, &path, args)),
        _ => (program.into(), args.to_vec()),
    }
}

/// Checked once: is there `systemd-run` and a user manager to talk to?
/// `CLAWSH_SCOPES=off` skips scopes altogether.
fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        if std::env::var("CLAWSH_SCOPES").is_ok_and(|v| v == "off") {
            return false;
        }
        Command::new("systemd-run")
            .args(["--user", "--scope", "--quiet", "--collect", "--", "true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

fn scope_args(clawsh_pid: u32, id: usize, program: &Path, args: &[String]) -> Vec<String> {
    let mut out = vec![
        "--user".to_owned(),
        "--scope".to_owned(),
        "--quiet".to_owned(),
        // Gone once its processes are, even if the program failed.
        "--collect".to_owned(),
        format!("--unit=clawsh-{clawsh_pid}-{id}"),
        "--".to_owned(),
        program.to_string_lossy().into_owned(),
    ];
    out.extend_from_slice(args);
    out
}

/// Where `program` would run from: as given if it has a slash, otherwise the
/// first executable match on `path`.
fn find_program(program: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let runnable = |p: &Path| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if program.contains('/') {
        let p = PathBuf::from(program);
        return runnable(&p).then_some(p);
    }
    std::env::split_paths(path?).map(|dir| dir.join(program)).find(|p| runnable(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_is_named_after_clawsh_and_the_pane() {
        let args = scope_args(42, 7, Path::new("/usr/bin/claude"), &["--resume".into(), "abc".into()]);
        assert_eq!(
            args,
            ["--user", "--scope", "--quiet", "--collect", "--unit=clawsh-42-7", "--", "/usr/bin/claude", "--resume", "abc"]
        );
    }

    #[test]
    fn finds_programs_on_path_and_skips_missing_ones() {
        let path = std::env::join_paths(["/nonexistent", "/bin", "/usr/bin"]).unwrap();
        let sh = find_program("sh", Some(&path)).expect("sh is on PATH");
        assert!(sh.ends_with("sh"));
        assert_eq!(find_program("clawsh-no-such-program", Some(&path)), None);
        assert_eq!(find_program("sh", None), None);
        assert_eq!(find_program(&sh.to_string_lossy(), None), Some(sh));
        assert_eq!(find_program("/etc/hostname-nope", None), None);
    }
}
