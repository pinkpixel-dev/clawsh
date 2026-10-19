use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::event::AppEvent;

/// The write half of a PTY, shared with the emulator so it can answer
/// terminal queries (device attributes, kitty queries, size reports).
pub type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Env vars from the host terminal that would make Claude think it is talking
/// to kitty, Ghostty, tmux, etc. directly instead of to our emulator.
const HOST_ENV: &[&str] = &[
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "KITTY_PUBLIC_KEY",
    "KITTY_LISTEN_ON",
    "KITTY_INSTALLATION_DIR",
    "GHOSTTY_RESOURCES_DIR",
    "GHOSTTY_BIN_DIR",
    "GHOSTTY_SHELL_FEATURES",
    "TERM_PROGRAM_VERSION",
    "WEZTERM_PANE",
    "WEZTERM_EXECUTABLE",
    "TMUX",
    "TMUX_PANE",
    "ZELLIJ",
    "STY",
    // Markers a running Claude session sets for its children. Each tab is a
    // top-level session, even when clawsh was started from inside one.
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_PID",
];

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: SharedWriter,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    /// Spawns `program` in a new PTY and starts a reader thread that forwards
    /// its output to the app as `AppEvent::PtyOutput`.
    pub fn spawn(
        id: usize,
        program: &str,
        args: &[String],
        env: &[(&str, String)],
        cwd: &Path,
        size: PtySize,
        tx: Sender<AppEvent>,
    ) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(size)
            .context("could not open a pty")?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(cwd);
        for key in HOST_ENV {
            cmd.env_remove(key);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "clawsh");
        cmd.env("CLAWSH", "1");
        for (key, value) in env {
            cmd.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("could not start `{program}`"))?;
        // The child holds its own copy of the slave side now.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer()?));

        thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(AppEvent::PtyOutput(id, buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(AppEvent::PtyExited(id));
        });

        Ok(Self {
            master: pair.master,
            writer,
            child,
        })
    }

    pub fn writer(&self) -> SharedWriter {
        Arc::clone(&self.writer)
    }

    pub fn write(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(data);
            let _ = w.flush();
        }
    }

    pub fn resize(&self, size: PtySize) {
        let _ = self.master.resize(size);
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
