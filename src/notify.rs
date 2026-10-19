//! Desktop notifications for tabs you aren't looking at: Claude waiting on
//! you, finishing, or failing. Sent with `notify-send`, so they show up in
//! whatever notification center the desktop has. `ctrl+b m` turns them off.

use std::process::{Command, Stdio};
use std::thread;

use crate::status::Status;

/// The title and body for a status worth interrupting someone for.
pub fn message(status: Status, workspace: &str, tab: &str) -> Option<(String, String)> {
    let what = match status {
        Status::NeedsInput => "is waiting on you",
        Status::Done => "finished",
        Status::Failed => "hit an error",
        Status::Idle | Status::Working => return None,
    };
    // Some notification daemons read the body as markup, and titles come
    // from whatever the program in the tab set.
    Some((format!("Claw$h · {}", escape(workspace)), format!("{} {what}", escape(tab))))
}

/// Shows a notification without blocking the app. A missing `notify-send`
/// just means no notification.
pub fn send(title: String, body: String) {
    thread::spawn(move || {
        let _ = Command::new("notify-send")
            .args(["--app-name=Claw$h", "--icon=utilities-terminal", &title, &body])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_news_gets_a_notification() {
        let (title, body) = message(Status::NeedsInput, "quota", "fix auth").unwrap();
        assert_eq!(title, "Claw$h · quota");
        assert_eq!(body, "fix auth is waiting on you");
        assert_eq!(message(Status::Done, "q", "t").unwrap().1, "t finished");
        assert!(message(Status::Working, "q", "t").is_none());
        assert!(message(Status::Idle, "q", "t").is_none());
    }

    #[test]
    fn titles_cannot_inject_markup() {
        let (_, body) = message(Status::Failed, "q", "<b>x</b> & y").unwrap();
        assert_eq!(body, "&lt;b&gt;x&lt;/b&gt; &amp; y hit an error");
    }
}
