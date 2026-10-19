use std::sync::mpsc::Sender;
use std::thread;

/// Everything the main loop reacts to, funneled through one channel.
pub enum AppEvent {
    Input(crossterm::event::Event),
    PtyOutput(usize, Vec<u8>),
    PtyExited(usize),
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
