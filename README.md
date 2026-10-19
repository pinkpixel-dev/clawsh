<p align="center">
  <img src="icon.png" alt="Claw$h icon" width="300" height="300">
</p>

# Claw$h

Claw$h is a terminal workspace for Claude Code. Each tab runs a real `claude` process, and Claw$h wraps it in a ratatui interface with room for workspaces, tabs, and status, kind of like herdr but built around Claude.

I mostly use Claude, so the goal is to keep everything Claude already does (including mods and their kitty graphics) working exactly as it does in a normal terminal, and add the small things around it that I like having: per-workspace status, usage limits, and quick switching.

The command is `clawsh` (the `$` only lives in the name, since shells would read `$h` as a variable). On crates.io the package is `clawsh-tui`.

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
| `a` | Sort the sidebar by activity, or back to open order |
| `[` | Copy mode: select text with the keyboard, including the scrollback |
| `r` | Review what changed in this tab's folder and comment on it |
| `,` | Rename the current tab |
| `t` | Open a workspace template |
| `T` | Save the current workspace as a template |
| `i` | Insert a prompt snippet into the current tab |
| `q` | Quit |
| `ctrl+b` | Send a literal `ctrl+b` to Claude |
| `esc` | Cancel |

### Tab names

Tabs name themselves. Claw$h uses the title Claude gives its session (without the little status glyph in front), and when there isn't one yet, it checks the session's transcript: a name you set with `/rename`, then the title Claude generated, then the first thing you typed. That last one is what keeps a tab you only ran slash commands in from being stuck as `claude` forever.

If you'd rather pick the name yourself, press `ctrl+b` then `,`. The current name is filled in so you can tweak it, `Enter` saves, and `esc` cancels. A name you set sticks, even across restarts. Save an empty name to hand the tab back to auto-naming.

### Workspace templates

If you open the same project with the same set of tabs over and over, save it as a template. Set things up the way you like, press `ctrl+b` then `T`, type a name (the folder's name is filled in), and press `Enter`. Later, `ctrl+b` then `t` opens a picker of your templates, and picking one opens its folder as a new workspace with all its tabs.

Templates are plain TOML files in `~/.config/clawsh/templates/` (or `$XDG_CONFIG_HOME/clawsh/templates/`), one per template, so you can also write or tweak them by hand:

```toml
name = "slate"
path = "~/code/slate"

[[tabs]]
name = "main"

[[tabs]]
name = "review"
args = ["--model", "opus"]
prompt = "Read DOCS/ROADMAP.md and tell me what's next"
```

Each tab can have a `name` (same as setting one with `ctrl+b ,`), extra `claude` flags in `args`, and a `prompt` that gets sent as the first message when the tab starts. All three are optional. A template with no tabs opens one plain tab.

A few things to know:

- Templates always start new conversations. Saving one records the folder, the names you gave tabs, and the flags and prompt of tabs that came from a template, but never which conversation a tab was in.
- Saving under a name that already exists replaces that template.
- If the template's folder is already open as a workspace, Claw$h just switches to it, the same as opening that folder from `ctrl+b w`.
- A file that doesn't parse still shows up in the picker with its error, so it doesn't just quietly disappear.

### Prompt snippets

Snippets are prompts you type a lot, saved as files. Press `ctrl+b` then `i`, pick one, and it gets pasted into the current tab. Enter isn't pressed for you, so you can tweak the text before sending it.

Each snippet is a Markdown file. The file name is the snippet's name, and the whole file is the prompt, so multi-line prompts are fine:

- `~/.config/clawsh/snippets/` (or `$XDG_CONFIG_HOME/clawsh/snippets/`) for snippets you want everywhere
- `.clawsh/snippets/` inside a workspace's folder for ones that only make sense in that project

The picker lists the workspace's snippets first, then the global ones, each labeled `repo` or `global` with a preview of its first line. If both folders have a snippet with the same name, you'll see both. Blank lines at the top and the final newline are dropped, so pasting a snippet never sends it by accident. If Claude is waiting on you (a permission prompt, say), Claw$h won't paste and tells you to answer Claude first.

Claude Code's own custom slash commands (`.claude/commands/`) are still the better fit for a prompt you want to run as a command. Snippets are for text you want to drop in and edit, in any tab.

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

In the sidebar, tab titles take their status color too. Idle tabs fade as they sit: for the first 15 minutes after a turn they look just stopped, then they go muted, and after 2 hours the row dims. That makes it pretty easy to tell the tab you just used from the one you forgot about on Tuesday.

Press `ctrl+b` then `a` to sort the sidebar by activity: whatever's waiting on you first, then working tabs, then the rest by how recently they did something, with workspaces ordered by their busiest tab. It only changes the sidebar. The tab bar and `ctrl+b 1` to `9` keep the order you opened things in, so the number keys never move under you. Press it again for open order. Like notifications, the choice is remembered.

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

### Reviewing changes

Press `ctrl+b` then `r` to see what changed in the current tab's folder. The review takes over the tab: changed files on the left with their `+`/`-` counts, and the selected file's diff on the right. It's a git thing, so the folder has to be in a git repo.

There are two scopes. **Uncommitted** (`u`) is everything that differs from `HEAD`, untracked files included. **Last turn** (`t`) is only what changed since you last sent this tab a prompt, which is usually the part you actually want to check. When Claude finishes a turn while you're looking, the review reloads on its own.

To leave feedback, move to a line (or select a few with `v`), press `c`, type, and hit `Enter`. Commented lines get a yellow `▌`. When you're done, `s` pastes every comment into that tab's Claude as one message, with the lines quoted under their file and line numbers. It doesn't press Enter for you, so you can add to it before sending. If Claude is sitting on a permission prompt, `s` waits, since the paste would get lost.

| Key | What it does |
|---|---|
| `u` / `t` | Uncommitted / last turn |
| `tab` | Switch between the file list and the diff |
| `j` / `k` or arrows | Move (files or diff lines) |
| `f` / `F` | Next / previous file, from anywhere |
| `]` / `[` | Next / previous hunk |
| `ctrl+d` / `ctrl+u`, `PgDn` / `PgUp` | Half a page / a page |
| `v` | Start (or drop) a selection |
| `c` or `Enter` | Comment on the selection, or edit the comment under the cursor |
| `d` | Delete the comment under the cursor |
| `n` / `N` | Next / previous comment |
| `m` | Show a markdown file rendered instead of as a diff |
| `s` | Send all comments to Claude |
| `r` | Reload |
| `q` or `esc` | Close the review |

The mouse works too: click a file or a scope, click a diff line, drag to select lines, and scroll either side. On narrow terminals the file list and the diff take turns, and `tab` flips between them.

Comments stay with the tab until you send them, so closing the review and coming back keeps them. They aren't saved when you quit Claw$h, though.

A couple of things to know. Renamed files show up as a delete plus an add. The last-turn snapshot runs inside Claude's prompt hook, so in a very big repo it adds a little delay before each prompt (it took 18 ms on this repo). Both scopes write snapshot objects into the repo's `.git` folder. Git cleans those up on its own eventually, since nothing points at them.

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
