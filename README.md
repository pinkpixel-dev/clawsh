# Claw$h

Claw$h is a terminal workspace for Claude Code. Each tab runs a real `claude` process, and Claw$h wraps it in a ratatui interface with room for workspaces, tabs, and status, kind of like herdr but built around Claude.

I mostly use Claude, so the goal is to keep everything Claude already does (including mods and their kitty graphics) working exactly as it does in a normal terminal, and add the small things around it that I like having: per-workspace status, usage limits, and quick switching.

The command is `clawsh` (the `$` only lives in the name, since shells would read `$h` as a variable). On crates.io the package is `clawsh-tui`, because `clawsh` was already taken there.

## Status

This is early, but it's usable. You get workspaces (one per folder) with as many Claude tabs as you want in each, a sidebar and tab bar you can click, and kitty images from Claude and its mods passed through to your terminal. Status badges and usage limits are still in progress. See [DOCS/ROADMAP.md](DOCS/ROADMAP.md) for the plan.

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

That folder becomes your first workspace. Anything after `clawsh` is passed to that first `claude`, so `clawsh --resume` works like `claude --resume`.

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
| `q` | Quit |
| `ctrl+b` | Send a literal `ctrl+b` to Claude |
| `esc` | Cancel |

You can also click workspaces and tabs in the sidebar, tabs in the tab bar, `+` for a new tab, and `+ new workspace`.

The folder picker lists the folder you launched from plus its sibling folders. Type to fuzzy filter, or type a full path (starting with `/` or `~`) to open any folder. If you keep projects somewhere else too, add those parent folders to `CLAWSH_PROJECTS`, separated by colons:

```bash
CLAWSH_PROJECTS=~/work:~/oss ./target/release/clawsh
```

Picking a folder that's already open just switches to it. When the last tab closes, Claw$h quits.

To try it with something other than Claude, set `CLAWSH_PROGRAM`:

```bash
CLAWSH_PROGRAM=bash ./target/release/clawsh
```

## License

Apache 2.0. See [LICENSE](LICENSE).

Made with 💖 by [Pink Pixel](https://pinkpixel.dev)
