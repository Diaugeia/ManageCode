# ManageCode

> **The unified dashboard for your AI coding agents.** One panel to manage every
> Claude Code and Codex session on your machine — see global run state at a
> glance, and move records and memory between tools and directories.

**English** · [简体中文](README.zh-CN.md)

```
┌─ ManageCode ── 49 sessions · 1 active · ▶ 2 tmux · ▷ 3 codex · $2916.45 ──┐
│ ▾ ~/Project/05_2026/MinionsCode                                ▶1  ●1   3  │
│    ▶ tmux busy   rust-tui notify integration   sonnet-4.6  $  2.41   2m   │
│    ● idle        refresh strategy notes         opus       $  0.47  14h   │
│ ▾ ~/Project/03_2026/Forecasting_Reasoning                              5  │
│    ○ ended       backtest harness               gpt-5.5    $  3.10   3d   │
│    ○             清理 Zone 的无用 file           opus       $  0.12  17d   │
└────────────────────────────────────────────────────────────────────────────┘
```

## Why ManageCode

The Vibe Coding era runs on agents — Claude Code, Codex, and more. People keep
**dozens** of agent processes alive at once, and today the only "manager" is a
wall of `tmux` panes with no global picture.

ManageCode is the missing layer:

1. **One platform for every agent.** Claude Code *and* Codex sessions in a single
   list — live status, cost, model, working directory — instead of scattered,
   disconnected tools.
2. **Global situational awareness.** Watch the run state of all your agents on one
   panel: which are busy, which are idle and waiting for you, what each is
   spending. Press `Enter` and the session opens *inside* the dashboard.
3. **Records and memory that move with you.** Agents are usually tied to one
   directory — rename or relocate it and you lose the memory (`CLAUDE.md` /
   `AGENTS.md`). ManageCode migrates memory across directories *and* across
   tools, and converts a session record between Claude and Codex formats.

Written in **Rust**, configured with **TOML**, terminal-native performance, a
modern TUI — one static binary, no runtime.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Diaugeia/ManageCode/main/install.sh | bash
```

The script downloads the prebuilt binary for your platform and installs it to
`~/.local/bin/managecode`. No Rust toolchain required.

**Supported:** Linux x86_64, macOS Apple Silicon (M1 and newer).
**Intel Mac / Windows:** [build from source](#build-from-source) (Windows needs WSL).

Update later with `managecode --update` (or re-run the install command).
ManageCode also checks for a newer release on startup and shows an `⬆` hint;
silence it with `--no-update-check` or `update_check = false` in the config.

To uninstall, run:

```bash
curl -fsSL https://raw.githubusercontent.com/Diaugeia/ManageCode/main/uninstall.sh | bash
```

This removes the binary from `~/.local/bin`. Add `--purge` to also delete the
config directory (`~/.managecode`); `~/.claude` and `~/.codex` are never touched.

## First run

```bash
managecode
```

Every session on this machine appears — live at the top, recent below, then the
rest of the last 30 days. Pick one, press `Enter`, and it opens **right inside
the dashboard**: the list shrinks to a sidebar and a live terminal runs the
agent next to it. `Ctrl-a` hops back to the sidebar (the session keeps running),
pick another, jump back in later — all in one window.

Press `?` any time for the full, always-up-to-date keymap.

## Highlights

- **Multi-agent, one list.** Claude Code (`~/.claude`) and OpenAI Codex
  (`~/.codex`) sessions side by side, each priced with the right rates.
- **Persistent multi-session.** With `tmux`, launches run in detached background
  sessions you can switch in and out of all day — see
  [tmux-and-pty.md](docs/tmux-and-pty.md).
- **Group, tree, or flat.** `T` cycles the sidebar between grouped-by-`cwd`, a
  path-compressed **directory tree**, and a flat list.
- **Cost at a glance.** Per-session token usage and dollar cost, a daily total,
  a budget alert (`:`), and a `c` cost summary by directory / model / day.
- **Interop.** `x` converts a record to the other tool's format; `X` migrates a
  directory's memory (`CLAUDE.md` / `AGENTS.md`) to any other directory.
- **AI search & auto-naming.** `/` filters; if nothing matches, `Enter` runs a
  model search. `A` names unnamed sessions. Model is configurable.
- **Mouse-friendly, terminal-native colors**, desktop notifications when a busy
  session goes idle, and a fully **remappable keymap**.

## Documentation

- [Keybindings](docs/keybindings.md) — every key, and how to remap them
- [Configuration](docs/config.md) — the `~/.managecode/config.toml` reference
- [tmux vs. PTY](docs/tmux-and-pty.md) — how launches run and persist

## Build from source

```bash
git clone https://github.com/Diaugeia/ManageCode.git
cd ManageCode
./build.sh        # or: cargo build --release
```

Requires a recent stable Rust toolchain. The binary lands in `target/release/managecode`.

## License

MIT — see [LICENSE](LICENSE).
