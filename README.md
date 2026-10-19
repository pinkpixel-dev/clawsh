# Claw$h

Claw$h is a terminal workspace for Claude Code. Each tab runs a real `claude` process, and Claw$h wraps it in a ratatui interface with room for workspaces, tabs, and status, kind of like herdr but built around Claude.

I mostly use Claude, so the goal is to keep everything Claude already does (including mods and their kitty graphics) working exactly as it does in a normal terminal, and add the small things around it that I like having: per-workspace status, usage limits, and quick switching.

The command is `clawsh` (the `$` only lives in the name, since shells would read `$h` as a variable). On crates.io the package is `clawsh-tui`, because `clawsh` was already taken there.

## Status

This is early. Right now it runs a single Claude session in a pane with a footer bar, and kitty images from Claude and its mods are passed through to your terminal. Workspaces, tabs, status badges, and usage limits are still in progress. See [DOCS/ROADMAP.md](DOCS/ROADMAP.md) for the plan.

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

Anything after `clawsh` is passed to `claude`, so `clawsh --resume` works like `claude --resume`.

| Keys | What it does |
|---|---|
| `ctrl+b` then `q` | Quit |
| `ctrl+b` then `ctrl+b` | Send a literal `ctrl+b` to Claude |
| `ctrl+b` then `esc` | Cancel |

To try it with something other than Claude, set `CLAWSH_PROGRAM`:

```bash
CLAWSH_PROGRAM=bash ./target/release/clawsh
```

## License

Apache 2.0. See [LICENSE](LICENSE).

Made with 💖 by [Pink Pixel](https://pinkpixel.dev)
