# ManageCode

> A terminal dashboard for managing every Claude Code session on your machine.

**English** · [简体中文](README.zh-CN.md)

```
┌─ ManageCode ──── 49 sessions · 1 active · ▶ 2 tmux · $2916.45 total ──────┐
│ ▾ ~/Project/05_2026/MinionsCode                                ▶1  ●1   3  │
│    ▶ tmux busy   rust-tui notify integration   sonnet-4.6  $  2.41   2m   │
│    ● idle        refresh strategy notes         opus       $  0.47  14h   │
│ ▾ ~/Project/03_2026/Forecasting_Reasoning                              5  │
│    ▶ tmux idle   Q4 forecasting backtest        opus       $  0.75   3d   │
│    ○                清理 Zone 的无用 file        opus       $  0.12  17d   │
└────────────────────────────────────────────────────────────────────────────┘
```

One binary. Pick a session, press `Enter`, and it opens **right inside the
dashboard** — the list shrinks to a sidebar and a live terminal runs `claude`
next to it. Press `Ctrl-a` to hop back to the sidebar (the session keeps
running), pick another one, jump back in later — all in a single window, no
full-screen takeover.

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Minions-Land/ManageCode/main/install.sh | bash
```

That's it. The script downloads the prebuilt binary for your platform and
installs it to `~/.local/bin/managecode`. No Rust toolchain required.

**Supported:** Linux x86_64, macOS Apple Silicon (M1 and newer).
**Intel Mac / Windows:** [build from source](#build-from-source) (Windows needs WSL).

Tip: if `~/.local/bin` isn't on your `$PATH`, the installer tells you what
to add to your shell rc.

To update later, just re-run the same command — it always pulls the latest
release.

## First run

```bash
managecode
```

You'll see every session on this machine — currently alive at the top,
recent below, then everything else from the last 30 days. Press `?` any
time for the full keymap.

The basics (vim-style):

| Key | What happens |
|-----|--------------|
| `↑` / `↓` or `j` / `k` | move the selection |
| `Enter` | resume the highlighted session in the embedded terminal |
| `i` / `l` | jump focus into the terminal pane |
| `Ctrl-a` | from the terminal, return focus to the sidebar (configurable) |
| `n` | start a fresh `claude` in that session's directory |
| `s` | drop into a shell in that directory |
| `/` | filter by name / path; `Enter` falls back to AI search if nothing matches |
| `:` | settings (change the terminal escape prefix) |
| `q` | quit |

The mouse works too: click / drag / scroll inside the terminal pane is
forwarded to `claude` (or tmux); in the sidebar the wheel moves the selection
and a click on the pane focuses it.

## Multi-session, made simple

If you have `tmux` installed (Homebrew: `brew install tmux`, apt:
`sudo apt install tmux`), ManageCode automatically wraps every session
in a managed background process. The terminal pane attaches to it, so:

1. `Enter` on session A → talk for a while → `Ctrl-a` back to the sidebar
2. Session A stays marked `▶` (running in the background).
3. `Enter` on session B → talk to a different model in a different repo
   → `Ctrl-a` again.
4. Both are running. `Enter` on A re-attaches *exactly* where you left
   off. Switch back and forth all day — never leaving the dashboard.

To force-end a backgrounded session: select it, press `K`, confirm.

Without `tmux`, ManageCode still works — the embedded terminal just runs
claude directly. Exit claude the normal way (`/exit`, `Ctrl-D`) and the
pane closes back to the dashboard.

## Highlights

- **Live status.** Color-coded per session: green = idle, amber = busy,
  purple = thinking, teal `▶` = running in the background. Updates show
  up in well under a second.
- **Cost at a glance.** Per-session token usage and dollar cost, plus a
  daily total in the header.
- **Group by directory.** Working on multiple repos? Each row is grouped
  under its `cwd`; collapse the ones you don't care about with `space`.
- **AI search.** `/` is a normal substring filter; if nothing matches,
  `Enter` falls back to a Haiku call that searches across session
  content. `\` forces AI search directly.
- **Auto-naming.** `A` asks Haiku to give your unnamed sessions short,
  meaningful titles based on what was discussed.
- **Notifications.** When a backgrounded `busy` session goes back to
  `idle` (i.e., Claude is waiting for you), you get a desktop banner.
  Mute with `M`.
- **Launch options.** `N` opens a form to start `claude` with
  `--model`, `--dangerously-skip-permissions`, `--sandbox`, `--verbose`,
  `--add-dir` toggles.

## Keys reference

**Navigation**

| Key | Action |
|-----|--------|
| `↑` `↓` / `j` `k` | move |
| `g` / `G` | first / last |
| `space` / `tab` | collapse / expand the current group |
| `o` / `O` | collapse inactive / expand all groups |
| `T` | toggle directory grouping |

**Sessions & terminal**

| Key | Action |
|-----|--------|
| `Enter` | resume / re-attach in the embedded terminal |
| `i` / `l` | focus the terminal pane |
| `Ctrl-a` | from the terminal, return focus to the sidebar (configurable) |
| `n` | new `claude` in that cwd (defaults) |
| `N` | new `claude` with options form (editable cwd + recent-dir quick-pick) |
| `s` | new shell in that cwd |
| `r` | rename |
| `K` | kill the background tmux session for this row |

**Cost & settings**

| Key | Action |
|-----|--------|
| `c` | cost summary (by directory / model / day) |
| `:` | settings — terminal escape prefix and a daily budget alert |

**Search**

| Key | Action |
|-----|--------|
| `/` | literal filter |
| `\` | force AI search |
| `A` | auto-name unnamed sessions |

**Maintenance**

| Key | Action |
|-----|--------|
| `D` | delete junk sessions (tmp / empty) |
| `E` | delete sessions with no messages |
| `M` | toggle notifications |
| `R` | refresh now |
| `?` | this help |
| `q` / `Ctrl-C` | quit |

## FAQ

**Where does it find sessions?** It reads `~/.claude/sessions/` (live
PIDs) and `~/.claude/projects/<cwd>/*.jsonl` (conversation history) —
the same files Claude Code itself writes.

**Will it slow my machine down?** No. It uses a file watcher
(inotify / FSEvents) and only re-reads files that actually changed.
The CPU footprint when idle is essentially zero.

**Does it talk to any servers?** Only when you press `\` or `A`, which
invoke `claude --print --model haiku` locally — and only Anthropic's
API sees those queries. The dashboard itself is entirely local.

**Can I point it at a different `claude` binary?** Yes:
`CLAUDE_BIN=/path/to/claude managecode`. It also auto-discovers
`/opt/homebrew/bin/claude`, `/usr/local/bin/claude`,
`~/.claude/local/bin/claude`, `~/.local/bin/claude`, and `$PATH`.

**How do I uninstall?** `rm ~/.local/bin/managecode`. That's all.

## Configuration knobs

```bash
managecode --days 7        # only look back 7 days (default 30)
managecode --list          # print sessions to stdout, no TUI
managecode --version
INSTALL_DIR=/usr/local/bin VERSION=v0.2.0 bash install.sh
CLAUDE_BIN=/opt/homebrew/bin/claude managecode
```

Persistent state: custom session names go to
`~/.managecode/session-names.json`. That's the only file ManageCode
writes.

## Build from source

```bash
git clone https://github.com/Minions-Land/ManageCode.git
cd ManageCode
./build.sh
```

Requires Rust 1.74+. The build script compiles a release binary and
installs it to `~/.local/bin/managecode` (override with
`PREFIX=/usr/local`).

## License

MIT — see [LICENSE](LICENSE).
