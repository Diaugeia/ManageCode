# MinionsCode

A cross-platform TUI dashboard for [Claude Code](https://claude.com/claude-code) sessions.
Single static-ish Rust binary, no embedded terminal — selecting a session `exec`s
`claude --resume <id>` directly in your current tty, so the conversation keeps full
input fidelity (paste, mouse, ANSI, the works).

```
┌─ MinionsCode ──── 49 sessions · 1 active · $2916.45 total ─────────────────┐
│ ▾ ~/Project/05_2026/MinionsCode                                    ●1   3  │
│    ● busy   rust-tui refresh strategy           sonnet-4.6   $  2.41   2m  │
│    ○ rename session names persistence            opus        $  0.47  14h  │
│ ▾ ~/Project/03_2026/Forecasting_Reasoning                              5  │
│    ○ 清理 Zone 的无用 file                       opus        $  0.75   3d  │
│    ...                                                                     │
└────────────────────────────────────────────────────────────────────────────┘
```

## Install

One-liner (no Rust toolchain needed):

```bash
curl -fsSL https://raw.githubusercontent.com/ChengAoShen/MinionsCode/main/install.sh | bash
```

Downloads the prebuilt binary for your platform from
[Releases](https://github.com/ChengAoShen/MinionsCode/releases) and drops it
into `~/.local/bin/minionscode`. Set `INSTALL_DIR=/usr/local/bin` or
`VERSION=v0.1.0` to override.

Prebuilt platforms:

- Linux x86_64
- macOS x86_64 (Intel)
- macOS aarch64 (Apple Silicon)

Windows users: run inside WSL, or build from source.

### Build from source

```bash
git clone https://github.com/ChengAoShen/MinionsCode.git
cd MinionsCode
./build.sh                            # cargo build --release + install to ~/.local/bin
```

Requires Rust 1.74+.

## Run

```bash
minionscode                # launch the TUI
minionscode --list         # non-interactive: print sessions and exit
minionscode --days 7       # only look back 7 days of history (default 30)
minionscode --version
```

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
| `⏎` | resume selected session (`claude --resume`) |
| `n` | new claude in the selected cwd (defaults) |
| `N` | new claude with an options form (model, dangerous, sandbox, verbose, add-dir) |
| `s` | new shell in the selected cwd |
| `r` | rename session (saved to `~/.minionscode/session-names.json`) |

**Search & AI**

| Key | Action |
|-----|--------|
| `/` | literal filter; `⏎` falls back to AI search if nothing matches |
| `\` | force AI search using the current filter buffer (calls `claude --print --model haiku`) |
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

| Color | Meaning |
|-------|---------|
| 🟢 `●` green | alive, `idle` (waiting for input) |
| 🟠 `●` amber | alive, `busy` (executing / tool call) |
| 🟣 `●` purple | alive, `thinking` (extended thinking) |
| 🟡 `●` gold | exited but recently active |
| ⚪ `○` muted | old, ended |

Status strings come directly from `~/.claude/sessions/<id>.json` — whatever Claude
Code writes is what you see.

## Refresh strategy

Three layers, designed so updates feel instant without hammering disk:

1. **File watcher** (`notify` crate — inotify / FSEvents / kqueue). Any change
   under `~/.claude/sessions/` or `~/.claude/projects/` triggers a debounced
   (~180 ms) re-scan.
2. **PID / status sweep** every ~1.5 s. Re-reads only the small `sessions/*.json`
   files and verifies PIDs via `kill -0` — picks up `busy ↔ idle` and
   process-died transitions without touching JSONL.
3. **Fallback full scan** every 30 s (5 s if the watcher failed to attach, e.g.
   on a filesystem without inotify support).

End-to-end, a status change in a live session typically shows up in well under
one second.

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
