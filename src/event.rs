use std::sync::mpsc::Sender;
use std::thread;

use crate::persist::SessionRef;
use crate::status::Status;
use crate::usage::Report;

/// Everything the main loop reacts to, funneled through one channel.
pub enum AppEvent {
    Input(crossterm::event::Event),
    PtyOutput(usize, Vec<u8>),
    PtyExited(usize),
    /// A Claude hook reported a pane's status.
    Status(usize, Status),
    /// A Claude hook reported which conversation a pane is in.
    Session(usize, SessionRef),
    /// A fresh reading of Claude's usage limits.
    Usage(Report),
}

/// Reads host terminal input on its own thread.
pub fn spawn_input_reader(tx: Sender<AppEvent>) {
    thread::spawn(move || {
        while let Ok(ev) = crossterm::event::read() {
            if tx.send(AppEvent::Input(ev)).is_err() {
                break;
            }
        }
    });
}
