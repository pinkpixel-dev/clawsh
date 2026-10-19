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

### 🗂️ Workspaces and tabs
- Workspaces are folders, each with its own Claude tabs
- Sidebar lists workspaces and their tabs, with a tab bar for the current workspace
- Folder picker with fuzzy search over the launch folder's siblings, typed paths, and `CLAWSH_PROJECTS`
- Click workspaces, tabs, `+`, and `+ new workspace`
- Hidden tabs keep running at full size, so switching never reflows them
- Closing a tab removes its images from the terminal; closing the last one quits

### 🚦 Status
- Each tab shows whether Claude is idle, working, waiting on you, done, or failed, in the sidebar and the tab bar
- Workspaces show their most urgent tab, so the one that needs you stands out
- Done and failed clear once you open or type in that tab
- Status comes from Claude Code hooks added per tab with `--settings`, next to your existing hooks

### ⌨️ Controls
- `ctrl+b` prefix: `c` new tab, `w` workspace, `x` close, `n`/`p` and `1`-`9` for tabs, `j`/`k` for workspaces, `s` sidebar, `q` quit, `ctrl+b` sends a literal `ctrl+b`
- Footer bar with the current workspace, tab, and key hints for the active mode
- The sidebar hides itself on narrow terminals

### 🧹 Maintenance
- `scripts/cargo.sh` builds with a local Zig 0.15 toolchain, which Ghostty requires
