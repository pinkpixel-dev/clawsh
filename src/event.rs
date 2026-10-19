use libghostty_vt::style::RgbColor;
use libghostty_vt::terminal::ColorScheme;

use crate::persist::SessionRef;
use crate::search::Hit;
use crate::status::Status;
use crate::usage::Report;

/// Everything the main loop reacts to, funneled through one channel.
pub enum AppEvent {
    Input(crossterm::event::Event),
    PtyOutput(usize, Vec<u8>),
    PtyExited(usize),
    /// A Claude hook reported a pane's status.
    Status(usize, Status),
    /// A pane's Claude was sent a prompt; this tree is the worktree as it
    /// was just before, for the review view's last-turn scope.
    Turn(usize, String),
    /// A Claude hook reported which conversation a pane is in.
    Session(usize, SessionRef),
    /// A conversation search found a match; the number says which search.
    SearchHit(u64, Box<Hit>),
    /// That conversation search looked at every transcript.
    SearchDone(u64),
    /// A fresh reading of Claude's usage limits.
    Usage(Report),
    /// The host terminal switched between light and dark (mode 2031).
    HostScheme(ColorScheme),
    /// The host terminal's reply to OSC 10 (foreground) or 11 (background).
    HostColor(u8, RgbColor),
}
