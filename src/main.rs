mod app;
mod event;
mod graphics;
mod host;
mod pane;
mod picker;
mod theme;
mod ui;
mod workspace;

use anyhow::Result;

fn main() -> Result<()> {
    // Everything after `clawsh` goes to Claude. CLAWSH_PROGRAM
    // swaps the program, which is handy for testing with a plain shell.
    let launch = app::Launch {
        program: std::env::var("CLAWSH_PROGRAM").unwrap_or_else(|_| "claude".into()),
        args: std::env::args().skip(1).collect(),
        cwd: std::env::current_dir()?,
    };

    let mut term = ratatui::init();
    let result = host::HostGuard::enter().and_then(|_guard| app::run(&mut term, launch));
    ratatui::restore();
    result
}
