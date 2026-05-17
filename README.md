# MinionsCode

A cross-platform TUI dashboard for [Claude Code](https://claude.com/claude-code)
sessions. One static-ish Rust binary, no embedded terminal — selecting a
session runs `claude --resume <id>` directly in your current tty (optionally
wrapped in a tmux session so it survives detach), so the conversation keeps
full input fidelity: paste, mouse, ANSI, the works.

```
┌─ MinionsCode ──── 49 sessions · 1 active · ▶ 2 tmux · $2916.45 total ──────┐
│ ▾ ~/Project/05_2026/MinionsCode                                ▶1  ●1   3  │
│    ▶ tmux busy   rust-tui notify integration   sonnet-4.6  $  2.41   2m   │
│    ● idle        refresh strategy notes         opus       $  0.47  14h   │
│ ▾ ~/Project/03_2026/Forecasting_Reasoning                              5  │
│    ▶ tmux idle   Q4 forecasting backtest        opus       $  0.75   3d   │
│    ○                清理 Zone 的无用 file        opus       $  0.12  17d   │
└────────────────────────────────────────────────────────────────────────────┘
```

## Install

One-liner — no Rust toolchain needed:

```bash
curl -fsSL https://raw.githubusercontent.com/ChengAoShen/MinionsCode/main/install.sh | bash
```

Downloads the prebuilt binary for your platform from
[Releases](https://github.com/ChengAoShen/MinionsCode/releases) and drops it
into `~/.local/bin/minionscode`. Honors `INSTALL_DIR=/usr/local/bin` and
`VERSION=v0.2.0` as overrides.

Prebuilt platforms:

- Linux x86_64
- macOS x86_64 (Intel)
- macOS aarch64 (Apple Silicon)

Windows users: run inside WSL.

### Build from source

```bash
git clone https://github.com/ChengAoShen/MinionsCode.git
cd MinionsCode
./build.sh                            # cargo build --release + install to ~/.local/bin
```

Requires Rust 1.74+.

### Optional: tmux

If `tmux` is on `$PATH`, MinionsCode automatically wraps every launched
session in a managed tmux session (prefix `mc-`). This is what gives you
multi-session: detach from one (`Ctrl-b d`) and you're back in the TUI
with the session still running in the background.

Without tmux, MinionsCode falls back to direct exec — one session at a
time, no detach. Same experience as before tmux integration.

## Usage

```bash
minionscode                # launch the TUI
minionscode --list         # non-interactive: print sessions and exit
minionscode --days 7       # only look back 7 days (default 30)
minionscode --version
```

## How sessions work

| What you do | What happens |
|---|---|
| Press `⏎` on a row with `●` | Launch `claude --resume` in a fresh tmux session `mc-<short-id>`, attach |
| Press `⏎` on a row with `▶` | Re-attach to the existing tmux session — picks up exactly where you left off |
| Inside claude: `Ctrl-b d` | Detach. The tmux session keeps running; you return to the TUI; the row now shows `▶` |
| Inside claude: `/exit` or `Ctrl-D` | Exit claude cleanly. tmux session ends. Row returns to `●` / `○` |
| `K` on a `▶` row | Force-kill the background tmux session (will SIGTERM claude) |
| `n` (new claude) | Same flow, but starts a fresh conversation in the selected `cwd` |
| `N` | Same as `n`, but pops up an options form (model / dangerous / sandbox / verbose / add-dir) first |
| `s` | Open the user's `$SHELL` in the selected `cwd`, also inside a managed tmux session |

So a full multi-session workflow looks like:

1. Resume session A → talk a bit → `Ctrl-b d` (now A is `▶ tmux idle` in the background)
2. `↓` to session B → `Enter` to resume → talk a bit → `Ctrl-b d`
3. Both are running in tmux now. `↑` back to A → `Enter` re-attaches. Repeat.

If MinionsCode is itself launched **inside** a tmux client, it skips its own
tmux wrapping (no nested tmux — that's a UX trap). You get the legacy
"one-at-a-time" behavior, but you can use your existing tmux's
window / pane switching for multi-tasking.

## Keys

**Navigation**

| Key | Action |
|-----|--------|
| `↑ ↓` / `j k` | navigate |
| `g` / `G` | first / last |
| `space` / `tab` | collapse / expand current group |
| `o` / `O` | collapse inactive / expand all groups |
| `T` | toggle grouping by directory |

**Session actions**

| Key | Action |
|-----|--------|
| `⏎` | resume / re-attach selected session |
| `n` | new claude in the selected cwd (defaults) |
| `N` | new claude with an options form |
| `s` | new shell in the selected cwd |
| `r` | rename (saved to `~/.minionscode/session-names.json`) |
| `K` | kill the background tmux session backing the selected row |

**Search & AI**

| Key | Action |
|-----|--------|
| `/` | literal filter; `⏎` falls back to AI search if nothing matches |
| `\` | force AI search using the current filter buffer (Haiku) |
| `A` | auto-name up to 12 unnamed sessions via Haiku |

**Maintenance**

| Key | Action |
|-----|--------|
| `D` | delete junk sessions (tmp / empty) |
| `E` | delete empty sessions |
| `M` | toggle desktop notifications |
| `R` | refresh now |
| `?` | help overlay |
| `q` / `Ctrl-C` | quit |

## Layout

Responsive — adapts to terminal size:

- **Wide** (≥ 110 cols): list + detail side-by-side
- **Stacked** (≥ 70 cols, ≥ 24 rows): list on top, compact detail below
- **Narrow** (smaller): list only; selected session summary collapses into the footer

## Status display

| Marker | Meaning |
|--------|---------|
| 🟢 `●` green | alive, `idle` (waiting for input) |
| 🟠 `●` amber | alive, `busy` (executing / tool call) |
| 🟣 `●` purple | alive, `thinking` (extended thinking) |
| 🟦 `▶` teal | session is running in a background tmux session (re-attachable) |
| 🟡 `●` gold | exited but recently active |
| ⚪ `○` muted | old, ended |

Status strings come directly from `~/.claude/sessions/<id>.json` — whatever
Claude Code writes is what you see. The `▶` overlay reflects `tmux ls`.

## Refresh strategy

Four layers, designed so updates feel instant without hammering disk:

1. **File watcher** (`notify` crate — inotify / FSEvents / kqueue) on
   `~/.claude/sessions/` and `~/.claude/projects/` → 180 ms debounce →
   re-scan.
2. **PID / status sweep** every ~1.5 s — re-reads only the small
   `sessions/*.json` files and verifies PIDs via `kill -0`.
3. **tmux poll** every ~2 s — `tmux list-sessions` to reconcile the
   background-session indicator.
4. **Fallback full scan** every 30 s (5 s if the watcher couldn't attach).

End-to-end, status changes typically show up in well under one second.

## Notifications

Fires a desktop notification when a live `claude` session transitions from
`busy` → `idle` after having been busy for ≥ 8 s, with a 30 s per-session
cooldown — designed to skip short tool turns and only signal completion of a
real conversation.

Backend:
- **macOS**: `osascript` (native banner)
- **Linux**: `notify-send`
- **everywhere**: terminal bell (`\x07`)

Toggle with `M` inside the TUI.

## What it reads

- `~/.claude/sessions/*.json` — live PIDs (`kill -0` to verify)
- `~/.claude/projects/<encoded-cwd>/*.jsonl` — per-session token usage,
  parsed and cached by `size:mtime`
- `tmux list-sessions -F '#S'` — to know which Claude sessions are in
  detachable background mode

Token costs use public Anthropic pricing (Opus / Sonnet / Haiku, inputs /
outputs / cache reads / cache writes).

## Custom claude binary

Auto-discovery checks in order:

1. `$CLAUDE_BIN`
2. `/opt/homebrew/bin/claude`
3. `/usr/local/bin/claude`
4. `~/.claude/local/bin/claude`
5. `~/.local/bin/claude`
6. `$PATH`

Set `CLAUDE_BIN=/path/to/claude` to override.

## License

MIT
