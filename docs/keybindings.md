# Keybindings

All Browse-mode keys are remappable from `[keys]` in
[`config.toml`](config.md) using the **action name** in the last column. The
in-app help overlay (`?`) is generated from the same table, so it always matches
your active bindings.

## Browse

**Navigation**

| Key | Action | action name |
|-----|--------|-------------|
| `↑` `↓` / `k` `j` | move selection | `up` / `down` |
| `PgUp` `PgDn` | jump 10 | `page_up` / `page_down` |
| `g` / `G` | first / last | `top` / `bottom` |
| `space` / `tab` | collapse / expand the current group | `toggle_group` |
| `o` / `O` | collapse inactive / expand all | `collapse_inactive` / `expand_all` |
| `za` / `zR` / `zM` | vim folds: toggle current · expand all · collapse all | — |
| `T` | cycle view: grouped → tree → flat | `cycle_view` |
| `b` | set the tree-view root directory (only sessions under it show) | `set_tree_root` |

**Sessions**

| Key | Action | action name |
|-----|--------|-------------|
| `Enter` | resume / re-attach in the embedded terminal | `open` |
| `i` / `l` | focus the terminal pane | `focus_terminal` |
| `n` | new `claude` in that cwd (defaults) | `new_claude` |
| `N` | new `claude` with the options form | `launch_form` |
| `s` | new shell in that cwd — a terminal, no `claude` | `new_shell` |
| `r` | rename | `rename` |
| `x` | convert record to the other tool (Claude ↔ Codex) | `convert` |
| `X` | migrate memory (CLAUDE.md / AGENTS.md) to another directory | `migrate_memory` |
| `K` | kill the background tmux session for this row | `kill_tmux` |

**Search & AI**

| Key | Action | action name |
|-----|--------|-------------|
| `/` | literal filter | `filter` |
| `\` | force AI search | `ai_search` |
| `A` | auto-name unnamed sessions | `auto_name` |

**Maintenance**

| Key | Action | action name |
|-----|--------|-------------|
| `D` | delete junk sessions (tmp / empty) | `delete_junk` |
| `E` | delete sessions with no messages | `delete_empty` |
| `M` | toggle desktop notifications | `toggle_mute` |
| `R` | refresh now | `refresh` |
| `:` | settings (escape prefix, daily budget) | `settings` |
| `c` | cost summary (by directory / model / day) | `cost_summary` |
| `?` | this help | `help` |
| `q` / `Ctrl-C` | quit | `quit` |

## Terminal pane

| Key | Action |
|-----|--------|
| `Ctrl-a` (configurable) | return focus to the sidebar (the session keeps running) |
| `Ctrl-b d` | detach the tmux session (when running under tmux) |

The sidebar↔terminal focus prefix is set by `escape_prefix` in the config.

## Other overlays

| Mode | Keys |
|------|------|
| Filter (`/`) | `Enter` apply (falls back to AI search) · `\` AI search · `Esc` cancel |
| Rename (`r`) | `Enter` save · `Esc` cancel |
| Launch form (`N`) | `Enter` launch · `space` toggle option · `←→` recent dirs · `Esc` cancel |
| Migrate memory (`X`) | `Enter` migrate · `←→` recent dirs · `Esc` cancel |
| Settings (`:`) | `Enter` save · `Esc` cancel |

## Remapping example

```toml
[keys]
quit = "Q"        # require Shift-q to quit
refresh = "f5"
convert = "ctrl-x"
```
