<p align="center">
  <img src="icon.png" alt="Claw$h icon" width="300" height="300">
</p>

# Claw$h

Claw$h is a terminal workspace for Claude Code. Each tab runs a real `claude` process, and Claw$h wraps it in a ratatui interface with room for workspaces, tabs, and status, kind of like herdr but built around Claude.

I mostly use Claude, so the goal is to keep everything Claude already does (including mods and their kitty graphics) working exactly as it does in a normal terminal, and add the small things around it that I like having: per-workspace status, usage limits, and quick switching.

The command is `clawsh` (the `$` only lives in the name, since shells would read `$h` as a variable). On crates.io the package is `clawsh-tui`, because `clawsh` was already taken there.

## Status

This is early, but it's usable. You get workspaces (one per folder) with as many Claude tabs as you want in each, a sidebar and tab bar you can click, and kitty images from Claude and its mods passed through to your terminal. Each tab shows what its Claude is doing, so you can tell at a glance which project needs you, and the bottom of the sidebar shows how much of your 5-hour and weekly Claude limits you have left. See [DOCS/ROADMAP.md](DOCS/ROADMAP.md) for the plan.

I've only run it on Linux so far. Image support will need kitty or Ghostty as the host terminal.

## Building

The terminal emulation comes from [libghostty-vt](https://docs.rs/libghostty-vt), which builds Ghostty from source with Zig. Ghostty needs **Zig 0.15.x** exactly, and newer Zig releases (like the 0.16 in Arch's repos) fail its version check.

1. Download Zig 0.15.2 from [ziglang.org/download](https://ziglang.org/download/) and extract it to `~/.local/share/zig-0.15.2`.
2. Build through the wrapper script, which puts that Zig first on `PATH`:

```bash
./scripts/cargo.sh build --release
```

If your Zig lives somewhere else, point the script at it:

```bash
CLAWSH_ZIG_DIR=/path/to/zig-0.15.2 ./scripts/cargo.sh build --release
```

The first build clones Ghostty into the build directory, so it needs network access and takes a minute.

## Usage

Run it from the folder you want Claude to work in:

```bash
./target/release/clawsh
```

That folder becomes your active workspace. Anything after `clawsh` is passed to Claude in that folder, so `clawsh --resume` works like `claude --resume`.

### Picking up where you left off

Claw$h remembers your workspaces, their tabs, and which conversation each tab was in. Next time you start it, everything comes back, and each tab reopens its conversation with `claude --resume`. A tab that never got a prompt just starts a fresh Claude, since there's nothing to resume yet. Folders that have been deleted are skipped, and the folder you launched from is added if it wasn't already open.

It's saved to `~/.local/state/clawsh/session.json` (or `$XDG_STATE_HOME/clawsh/session.json`) every time something changes, so closing the window or a crash doesn't lose anything. Closing every tab leaves you with a clean slate next time.

To skip the restore once and start clean:

```bash
./target/release/clawsh --fresh
```

`--fresh` is the only flag Claw$h keeps for itself. Everything else still goes to Claude.

Everything is behind a `ctrl+b` prefix, so Claude still gets all its own shortcuts:

| `ctrl+b` then | What it does |
|---|---|
| `c` | New Claude tab in the current workspace |
| `w` | Open a workspace (folder picker) |
| `x` | Close the current tab |
| `n` / `p` or `→` / `←` | Next / previous tab |
| `1` to `9` | Jump to a tab |
| `j` / `k` or `↓` / `↑` | Next / previous workspace |
| `s` | Hide or show the sidebar |
| `m` | Turn desktop notifications off or back on |
| `[` | Copy mode: select text with the keyboard, including the scrollback |
| `,` | Rename the current tab |
| `q` | Quit |
| `ctrl+b` | Send a literal `ctrl+b` to Claude |
| `esc` | Cancel |

### Tab names

Tabs name themselves. Claw$h uses the title Claude gives its session (without the little status glyph in front), and when there isn't one yet, it checks the session's transcript: a name you set with `/rename`, then the title Claude generated, then the first thing you typed. That last one is what keeps a tab you only ran slash commands in from being stuck as `claude` forever.

If you'd rather pick the name yourself, press `ctrl+b` then `,`. The current name is filled in so you can tweak it, `Enter` saves, and `esc` cancels. A name you set sticks, even across restarts. Save an empty name to hand the tab back to auto-naming.

### Tab status

Every tab has a little status icon in the sidebar and the tab bar, and each workspace shows its most urgent one:

| Icon | Meaning |
|---|---|
| `○` | Idle |
| spinner | Claude is working |
| `?` | Claude is waiting on you: a permission prompt or a question |
| `●` | Claude finished while you were looking somewhere else |
| `✗` | The turn failed (an API error, for example) |

`●` and `✗` clear once you switch to that tab or type in it.

When a tab you're *not* looking at starts waiting on you, finishes, or fails, Claw$h also sends a desktop notification (through `notify-send`), like "Claw$h · quota: fix auth is waiting on you". Press `ctrl+b` then `m` to turn them off or back on. That choice is remembered, even across `--fresh`.

This works through Claude Code hooks. Claw$h starts each Claude tab with `claude --settings '…'`, which adds a few hooks that run `clawsh __hook` and report back over a local socket. Those hooks are added next to your own hooks, not instead of them, and nothing in your Claude config gets changed. If you set `CLAWSH_PROGRAM` to something other than `claude`, the hooks are left off.

### Usage limits

The bottom of the sidebar shows your Claude 5-hour and weekly windows: a bar and the percent you have **left**, plus how long until each one resets. The bar is green normally, yellow under 25% left, and red under 10%.

Claw$h reads your Claude Code login from `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR`) and asks Anthropic's usage endpoint, the same one Claude Code uses for its own usage screen. Nothing gets written anywhere. It refreshes every 3 minutes, and sooner after a tab finishes a turn, but never more than once a minute. If it can't get a reading you'll see a short reason instead, like `usage: offline` or `usage: token expired, open Claude`.

Usage only shows when Claw$h is running Claude, so it stays out of the way with `CLAWSH_PROGRAM`.

### Light and dark terminals

When Claw$h starts, it asks your terminal for its real text and background colors and passes them on to every tab. So when Claude checks whether your terminal is light or dark to pick its theme, it gets the right answer.

Claw$h's own sidebar, tab bar, footer, and folder picker follow along too: a warm light palette on light terminals, the charcoal one on dark ones. If you switch your terminal's theme while Claw$h is running, it notices and switches its own palette, and every tab gets the new colors. That needs a terminal that announces theme changes (mode 2031); Ghostty does, and I believe recent kitty does as well. Claude itself picks its theme when it starts (as of Claude Code 2.1.303 it doesn't listen for theme changes), so a Claude that's already running keeps its theme until you restart it or change it with `/theme`. New tabs get the right one. If your terminal doesn't report its colors at all, Claw$h stays dark. To pin one yourself:

```bash
CLAWSH_THEME=light ./target/release/clawsh
```

### Copying from Claude

When Claude copies something (it uses the OSC 52 escape sequence), Claw$h passes it on to kitty or Ghostty, so it lands on your real clipboard just like it would outside Claw$h. Reading the clipboard isn't passed through, so nothing running in a tab can read what you've copied.

To copy text yourself, including stuff that's scrolled off the top, press `ctrl+b` then `[` for copy mode. The tab freezes and a pink cursor shows up where the program's cursor was:

| Key | What it does |
|---|---|
| arrows or `h` `j` `k` `l` | Move |
| `0` / `$` | Start / end of the line |
| `PgUp` / `PgDn`, `ctrl+u` / `ctrl+d` | A page / half a page |
| `g` / `G` | Top of the scrollback / the bottom |
| `v` or space | Start (or drop) a selection |
| `y` or `Enter` | Copy the selection and exit; with nothing selected, copies the line the cursor is on |
| `q` or `Esc` | Exit without copying |

In tabs where the program doesn't use the mouse itself (a plain shell, for example), you can also just drag across text. It's copied when you let go. Double-click copies the word under the pointer, and triple-click copies the whole line. Claude does its own mouse handling, so inside Claude, use its selection or copy mode. Holding `shift` while dragging still works too, but that's your terminal's own selection, so it covers the whole Claw$h window, sidebar included.

You can also click workspaces and tabs in the sidebar, tabs in the tab bar, `+` for a new tab, and `+ new workspace` at the top of the sidebar. When there are more workspaces than fit, the sidebar follows whichever tab you're on, and the mouse wheel scrolls it.

The folder picker lists the folder you launched from plus its sibling folders. Type to fuzzy filter, or type a full path (starting with `/` or `~`) to open any folder. If you keep projects somewhere else too, add those parent folders to `CLAWSH_PROJECTS`, separated by colons:

```bash
CLAWSH_PROJECTS=~/work:~/oss ./target/release/clawsh
```

Picking a folder that's already open just switches to it. When the last tab closes, Claw$h quits.

To try it with something other than Claude, set `CLAWSH_PROGRAM`:

```bash
CLAWSH_PROGRAM=bash ./target/release/clawsh
```

## Troubleshooting

**Images from Claude mods don't show up.** Claw$h hands image pixels to your terminal through shared memory (`/dev/shm`), which is quick and keeps animated mods cheap. It switches to plain base64 on its own over SSH. If your terminal still doesn't show images, try forcing base64:

```bash
CLAWSH_IMAGES=direct ./target/release/clawsh
```

## License

Apache 2.0. See [LICENSE](LICENSE).

`src/input/parse.rs` is adapted from [crossterm](https://github.com/crossterm-rs/crossterm)'s input parser and stays under its MIT license, which is included as `src/input/CROSSTERM-LICENSE`.

Made with 💖 by [Pink Pixel](https://pinkpixel.dev)
