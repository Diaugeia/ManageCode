# Configuration

ManageCode reads `~/.managecode/config.toml`. Every field is optional — an
absent file means defaults. A legacy `~/.managecode/config.json` from older
versions is migrated to TOML automatically on first run.

```toml
# ~/.managecode/config.toml — all fields optional; values shown are the defaults.

# --- General ---
default_view      = "grouped"   # initial sidebar layout: "grouped" | "tree" | "flat"
show_cost         = false       # show each session's cost, right-aligned (toggle in code only)
tree_root         = ""          # tree-view root; only sessions under it show ("" = launch cwd, set with `b`)
history_days      = 30          # how far back to scan (the --days flag overrides this)
# daily_budget_usd = 25.0       # header alert when today's spend crosses it (omit = off)
notifications     = true        # desktop banner when a busy session goes idle
update_check      = true        # check GitHub for a newer release on startup

# --- Sources ---
scan_claude       = true        # scan ~/.claude for Claude Code sessions
scan_codex        = true        # scan ~/.codex for OpenAI Codex sessions
claude_bin        = ""          # override the claude path ("" = $CLAUDE_BIN, then $PATH)
codex_bin         = ""          # override the codex path  ("" = $CODEX_BIN,  then $PATH)

# --- Terminal / tmux ---
escape_prefix        = "ctrl-a" # from the terminal pane, return focus to the sidebar
prefer_tmux          = true     # run launches in a detached tmux session (persist on detach)
cleanup_tmux_on_exit = true     # on quit, kill all mc-* tmux sessions this tool created

# --- AI (search + auto-name) ---
ai_model          = "haiku"     # model passed to `claude --print`
ai_timeout_secs   = 45          # per-call timeout

# --- Background refresh cadences ---
[refresh]
live_ms      = 1500             # PID / status sweep interval (ms)
tmux_ms      = 2000             # tmux backed-set reconcile interval (ms)
full_secs    = 30              # fallback full-scan interval with the watcher (s)
debounce_ms  = 180             # debounce after a file event before scanning (ms)
max_jsonl_mb = 100             # skip transcript files larger than this (MB)

# --- Key remaps (Browse mode) ---
[keys]
# Action name -> key. Action names are shown in the in-app help (?).
# quit = "x"
# refresh = "f5"
```

## Notes

- **`escape_prefix`** accepts forms like `"ctrl-a"`, `"f12"`, `"ctrl-space"`.
  `Ctrl-C` and `Ctrl-D` are reserved and cannot be bound.
- **`default_view`** maps to the `T` cycle (grouped → tree → flat). Invalid
  values fall back to `grouped`.
- **`prefer_tmux` / `cleanup_tmux_on_exit`** — see
  [tmux-and-pty.md](tmux-and-pty.md) for the full lifecycle.
- **`[keys]`** overrides only the letter/symbol key for an action; arrows, page
  keys, Enter and Tab keep their defaults. If two actions are mapped to the same
  key, the one listed first in the default table wins (deterministically). See
  [keybindings.md](keybindings.md) for the action names.
- **`claude_bin` / `codex_bin`** take priority over the `$CLAUDE_BIN` /
  `$CODEX_BIN` environment variables.

## Editing

Edit the file in any text editor; changes take effect on the next launch. The
in-app settings overlay (`:`) edits the escape prefix and daily budget and
writes them back to `config.toml`.
