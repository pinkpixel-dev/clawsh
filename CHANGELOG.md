# Changelog

## 0.1.0 - Unreleased

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

### ⌨️ Controls
- `ctrl+b` prefix: `c` new tab, `w` workspace, `x` close, `n`/`p` and `1`-`9` for tabs, `j`/`k` for workspaces, `s` sidebar, `q` quit, `ctrl+b` sends a literal `ctrl+b`
- Footer bar with the current workspace, tab, and key hints for the active mode
- The sidebar hides itself on narrow terminals
- The sidebar scrolls when it's full, follows the active tab, and scrolls with the mouse wheel

### 🧹 Maintenance
- Input is read by Claw$h itself, through a copy of crossterm's parser that also understands terminal replies, so theme notices and color replies can never turn into keystrokes
- `scripts/cargo.sh` builds with a local Zig 0.15 toolchain, which Ghostty requires
