mod app;
mod event;
mod graphics;
mod host;
mod pane;
mod persist;
mod shm;
mod picker;
mod status;
mod theme;
mod ui;
mod usage;
mod workspace;

use anyhow::Result;

fn main() -> Result<()> {
    // Claude runs `clawsh __hook` from its hooks; that never touches the terminal.
    if std::env::args().nth(1).as_deref() == Some(status::HOOK_ARG) {
        status::run_hook();
        return Ok(());
    }

    // `--fresh` skips restoring the last session; everything else after
    // `clawsh` goes to Claude. CLAWSH_PROGRAM swaps the program, which is
    // handy for testing with a plain shell.
    let (ours, args): (Vec<String>, Vec<String>) =
        std::env::args().skip(1).partition(|a| a == "--fresh");
    let launch = app::Launch {
        program: std::env::var("CLAWSH_PROGRAM").unwrap_or_else(|_| "claude".into()),
        args,
        cwd: std::env::current_dir()?,
        fresh: !ours.is_empty(),
    };

    let mut term = ratatui::init();
    let result = host::HostGuard::enter().and_then(|_guard| app::run(&mut term, launch));
    ratatui::restore();
    result
}
