# Changelog

## Unreleased

### 📱 Phone access (early)
- `ctrl+b W` (or Start phone access in the main menu) starts a small server inside the running Claw$h and copies a link with a one-time token. Pressing it again stops it
- The phone client in `web/` shows the same session at the phone's own size and sends keys, taps, scrolling, and pastes back
- Whichever screen typed last sets the pane size, so Claude reflows when you switch between the phone and the desk
- Compose mode (the default) gives the phone a real text field, so autocorrect and dictation work. Send or Return pastes the whole prompt and presses Enter. Keys mode sends every key as you type, for menus and `ctrl+b` commands
- The key bar is two rows that fit a phone's width, with a Compose/Keys toggle and an Enter key
- Release binaries and the crates.io package now include the phone client
- Starting it shows a QR code of the link, so you can just scan it with your phone
- With Tailscale running, it also starts `tailscale serve` so your phone gets an HTTPS link that works anywhere on your tailnet. It stops along with phone access, even if Claw$h crashes. Without Tailscale it stays on `127.0.0.1` and the popup says why
- `○ phone` in the footer shows it's on and turns into `● phone` while a phone is connected. Click it to see the QR code again
- It turns itself off after 15 minutes with no phone connected

## 1.2.1 - October 20, 2026

### 🐛 Fixes
- Each tab and drawer now starts in its own systemd scope, so when systemd-oomd steps in it kills only the tab that ran out of control instead of the whole terminal window with every workspace in it
- `CLAWSH_SCOPES=off` turns this off, and Claw$h starts tabs directly on its own when `systemd-run` or a user systemd session isn't available

## 1.2.0 - October 19, 2026

### 🎛️ Command palette
- `ctrl+b space` opens a fuzzy picker of commands, every tab, every workspace, snippets, and templates
- Commands show their `ctrl+b` key, so the palette doubles as a cheat sheet
- Copy mode is in the palette and the main menu too, for the pane that has the keyboard
- Picking a tab or workspace switches to it, and closing things from the palette still asks first
- Also in the main menu as Command palette, and listed first in the `ctrl+b` footer hints

### 📡 Radar
- `ctrl+b v` opens a list of every tab in every workspace, with whatever needs you first and then the most recently busy
- Each row shows the tab's status icon, title, workspace, and how long ago it last did something, plus a count of tabs per status at the bottom
- It stays live while open, so statuses and ages update as tabs change
- Enter or a click jumps to the tab, and `r` jumps there and opens the review on its last turn
- Also in the main menu as Radar, which puts it in the command palette too

### 🔎 Conversation search
- `ctrl+b /` searches every saved Claude conversation across all projects, not just the open tabs
- It matches your prompts and Claude's replies, ignoring case. Tool calls, tool output, and subagent transcripts are skipped
- Results stream in newest first, one per conversation, with its title, project folder, age, and the first matching line with the match underlined
- Enter or a click resumes it with `claude --resume` in its folder, opening the workspace if needed, or just switches if that conversation is already in a tab
- No index: each search reads the transcript files directly on background threads
- Also in the main menu as Search conversations, which puts it in the command palette too

### 📄 File picker
- `ctrl+b f` opens a fuzzy search over the workspace's files and pastes `@path` into Claude's input, without pressing enter
- In a git repo it lists tracked files plus untracked ones that aren't ignored. Elsewhere it walks the folder, skipping hidden folders, `node_modules`, and `target`. Either way it stops at 20,000 files.
- Matches at the start of a path segment rank higher, so `app` finds `src/app.rs` first
- Also in the main menu and the palette as Insert file

### 🌿 Git worktrees
- `ctrl+b g` lists the current repo's worktrees. Pick one to open it as a workspace, or pick `+ new branch` to make one.
- New worktrees go next to the repo as `<repo>-<branch>` (slashes become dashes). A branch that already exists gets checked out; a new one starts from whatever the current workspace has checked out.
- The sidebar shows each workspace's branch next to its name, with `⎇` in front for linked worktrees
- `ctrl+x` in the picker removes a worktree's folder. The branch stays. It won't touch the main checkout or a worktree that's open as a workspace, and if there are uncommitted changes the confirmation says how many will be lost.
- Also in the main menu and the palette as Worktrees

### 🐛 Fixes
- Quitting Claude with a double `ctrl+c` (or `/exit`) no longer closes the workspace. If it was the workspace's last tab, a fresh Claude starts in its place

## 1.1.0 - October 19, 2026

### 🖱️ Menus
- Right-click a tab (in the sidebar or the tab bar) for New tab, Rename, and Close
- Right-click a workspace for New tab, Rename, and Close
- Closing from a menu asks first, with Cancel selected so a stray enter is safe
- The `≡ Claw$h` button in the footer (or `ctrl+b o`) opens a menu with workspace, template, snippet, review, sidebar, sort, notification, and quit actions
- Menu items show their `ctrl+b` key, and pressing that key in the open menu picks the item
- Menus work with the mouse (hover to highlight, click to pick) and the keyboard (arrows or `j`/`k`, enter, esc)

### 🖥️ Terminal drawer
- Every tab can open its own shell in the workspace folder with `ctrl+b e` or the `>_` button at the right end of the tab bar
- It opens below Claude by default so Claude keeps its full width. The main menu can move drawers to the right side instead.
- Hiding a drawer keeps its shell running, so a dev server keeps going while you work in Claude
- `ctrl+b ;` or a click moves the keyboard between Claude and the drawer, and the divider turns pink while the drawer has it
- Drag the divider to resize. The side and size are saved with the session; drawers themselves start closed.
- Close a drawer by exiting its shell, or with Close terminal in the tab's right-click menu (which asks first)

### 🗂️ Workspaces
- Workspaces can be renamed. The name is display only, the folder stays the same, and it's saved with the session. An empty name goes back to the folder name.

## 1.0.0 - October 19, 2026

First public release.

### 🖥️ Pane
- Runs `claude` in a PTY with libghostty-vt doing the terminal emulation and ratatui drawing it
- Keys, mouse, paste, and focus events are encoded for whatever modes Claude turns on, including the kitty keyboard protocol
- Answers size, device attribute, and XTVERSION queries (XTVERSION reports Ghostty's VT core, which Claude needs before it shows images)
- Host terminal and parent Claude session env vars are removed from the child

### 🖼️ Images
- Kitty graphics from Claude and its mods show up inside the pane, in kitty and Ghostty
- Each pane's images get their own IDs on the host, so tabs can't overwrite each other's pictures
- PNG images sent with the kitty protocol are decoded
- Images Claw$h uploaded are removed from the host on exit
- Animated images (like the Clawd dashboard tile) no longer flash between frames
- Pixels go to the terminal through shared memory instead of base64 when the terminal is local, which cuts an animated mod from hundreds of KB per second to a few short commands; SSH and `CLAWSH_IMAGES=direct` still use base64

### 🗂️ Workspaces and tabs
- Workspaces are folders, each with its own Claude tabs
- Sidebar lists workspaces and their tabs, with a tab bar for the current workspace
- Folder picker with fuzzy search over the launch folder's siblings, typed paths, and `CLAWSH_PROJECTS`
- Click workspaces, tabs, `+`, and `+ new workspace` (at the top of the sidebar)
- Hidden tabs keep running at full size, so switching never reflows them
- Closing a tab removes its images from the terminal; closing the last one quits

### 🚦 Status
- Each tab shows whether Claude is idle, working, waiting on you, done, or failed, in the sidebar and the tab bar
- Workspaces show their most urgent tab, so the one that needs you stands out
- Done and failed clear once you open or type in that tab
- Status comes from Claude Code hooks added per tab with `--settings`, next to your existing hooks

### 📊 Usage
- The sidebar shows Claude's 5-hour and weekly limits: percent left, a colored bar, and time until reset
- Reads the local Claude Code login and refreshes every 3 minutes, plus after a turn finishes (at most once a minute)
- Shows a short reason instead when usage can't be read

### 💾 Sessions
- Workspaces, tabs, and each tab's Claude conversation are restored on the next launch with `claude --resume`
- Tabs that never got a prompt start fresh; deleted folders are skipped; the launch folder becomes active
- Saved to `~/.local/state/clawsh/session.json` whenever something changes
- `clawsh --fresh` starts clean once

### 🔔 Notifications
- Desktop notification when a background tab needs input, finishes, or fails
- `ctrl+b m` turns them off or on, and the choice is remembered

### 🌗 Light and dark
- Claw$h reads the terminal's real colors at startup and passes them to every tab, so Claude picks the right light or dark theme
- Claw$h's own chrome has a light palette now, chosen automatically on light terminals or with `CLAWSH_THEME=light`
- Switching the terminal between light and dark while running is picked up: Claw$h changes palette and passes the new colors to every tab, and programs that listen for theme notices get one (terminals that support mode 2031)

### 📋 Clipboard
- Copying from Claude (OSC 52) now reaches your real clipboard; clipboard reads stay blocked
- Copy mode (`ctrl+b [`): move a cursor through the screen and scrollback, select with `v`, copy with `y`
- Drag to select (copied on release) in tabs whose program doesn't use the mouse
- Double-click copies a word and triple-click copies a line in those same tabs
- Tabs name themselves from Claude's session title, falling back to the transcript (`/rename` name, Claude's generated title, then your first prompt)
- `ctrl+b ,` renames the current tab; your name sticks across restarts, and an empty name goes back to auto-naming
- Sidebar tab titles take their status color, and idle tabs fade with age (muted after 15 minutes, dimmed after 2 hours)

### 📐 Templates
- Workspace templates: a folder plus named tabs, each with optional `claude` flags and an opening prompt, stored as TOML in `~/.config/clawsh/templates/`
- `ctrl+b t` opens a template from a fuzzy picker, and `ctrl+b T` saves the current workspace as one
- Templates always start new conversations; opening one whose folder is already open just switches to it

### ✂️ Snippets
- Prompt snippets: Markdown files in `~/.config/clawsh/snippets/` or a workspace's `.clawsh/snippets/`
- `ctrl+b i` picks one and pastes it into the current tab without pressing Enter, workspace snippets first and each labeled `repo` or `global`
- `ctrl+b a` sorts the sidebar by activity, without changing tab numbers; the choice is remembered

### 🔍 Review
- `ctrl+b r` opens a review of the current tab's folder: changed files with `+`/`-` counts and a numbered diff, in place of the tab
- Two scopes: uncommitted changes (against `HEAD`, untracked files included) and the last turn (only what changed since the tab's last prompt)
- Comment on a line or a `v` selection, then `s` pastes all comments into that tab's Claude as one message; it holds off while Claude waits on a permission prompt
- Markdown files can be shown rendered with `m`
- Mouse support for files, scopes, lines, drag selection, and scrolling, and a one-column layout on narrow terminals
- The review reloads when the tab's turn finishes, and comments stay with the tab until they're sent

### ⌨️ Controls
- `ctrl+b` prefix: `c` new tab, `w` workspace, `x` close, `n`/`p` and `1`-`9` for tabs, `j`/`k` for workspaces, `s` sidebar, `q` quit, `ctrl+b` sends a literal `ctrl+b`
- Footer bar with the current workspace, tab, and key hints for the active mode
- The sidebar hides itself on narrow terminals
- The sidebar scrolls when it's full, follows the active tab, and scrolls with the mouse wheel

### 📦 Install
- Prebuilt Linux binaries for x86_64 and aarch64 on every GitHub release, built on Ubuntu 22.04 (glibc 2.35 or newer)
- `cargo binstall clawsh-tui` downloads the release binary instead of compiling
- Published to crates.io as `clawsh-tui`

### 🧹 Maintenance
- Input is read by Claw$h itself, through a copy of crossterm's parser that also understands terminal replies, so theme notices and color replies can never turn into keystrokes
- `scripts/cargo.sh` builds with a local Zig 0.15 toolchain, which Ghostty requires
- Release workflow builds, packages, and checksums the binaries when a `v*` tag is pushed

### 🏷️ Versioning
- 1.0.0
