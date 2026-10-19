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

### ⌨️ Controls
- `ctrl+b` prefix with `q` to quit and `ctrl+b` to send a literal `ctrl+b`
- Footer bar with the pane title and key hints

### 🧹 Maintenance
- `scripts/cargo.sh` builds with a local Zig 0.15 toolchain, which Ghostty requires
