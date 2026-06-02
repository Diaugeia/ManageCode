//! Keyboard and mouse event handlers. `handle_key` and `handle_mouse` are the
//! two entry points called from the main event loop; everything else dispatches
//! per UI mode.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, ConfirmAction, LaunchForm, Mode, PendingExec, RowHit};
use crate::config;
use crate::launcher;
use crate::ExitRequest;

pub fn handle_key(
    app: &mut App,
    code: KeyCode,
    mods: KeyModifiers,
) -> Option<ExitRequest> {
    // Terminal mode swallows ALL keys (including Ctrl-C, which must reach the
    // child) except the escape prefix. Checked before the global Ctrl-C quit.
    if matches!(app.mode, Mode::Terminal) {
        handle_terminal(app, code, mods);
        return None;
    }

    // Ctrl-C always quits, regardless of mode.
    if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        return Some(ExitRequest::Quit);
    }

    match app.mode {
        Mode::Browse => handle_browse(app, code, mods),
        Mode::Filter => {
            handle_filter(app, code);
            None
        }
        Mode::Rename => {
            handle_rename(app, code);
            None
        }
        Mode::Help => {
            if matches!(code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                app.mode = Mode::Browse;
            }
            None
        }
        Mode::Confirm(_) => {
            handle_confirm(app, code);
            None
        }
        Mode::Launch(_) => handle_launch(app, code),
        Mode::Settings => {
            handle_settings(app, code);
            None
        }
        Mode::CostSummary => {
            if matches!(
                code,
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('c') | KeyCode::Char('$')
            ) {
                app.mode = Mode::Browse;
            }
            None
        }
        // Reached only via the early return above; arm kept for exhaustiveness.
        Mode::Terminal => None,
    }
}

/// Route keys to the embedded terminal. The escape prefix (hardcoded `Ctrl-a`
/// for now; configurable in M4) returns focus to the sidebar without killing
/// the session.
fn handle_terminal(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    if app.config.escape_prefix.matches(code, mods) {
        app.blur_terminal();
        return;
    }
    if let Some(t) = app.term.as_mut() {
        t.reset_scrollback();
        t.send_key(code, mods);
    }
}

/// Mouse routing: forward to the embedded terminal when it's focused and the
/// pointer is over the pane; in the sidebar, the wheel moves the selection and
/// a click on the terminal pane focuses it.
pub fn handle_mouse(app: &mut App, me: crossterm::event::MouseEvent) {
    use crossterm::event::{MouseButton, MouseEventKind as K};

    let (px, py, pw, ph) = app.term_area.get();
    let over_pane = app.has_terminal()
        && me.column >= px
        && me.column < px.saturating_add(pw)
        && me.row >= py
        && me.row < py.saturating_add(ph);

    match app.mode {
        Mode::Terminal => {
            if over_pane {
                // Wheel scrolls our scrollback unless the child tracks the mouse.
                let tracking = app.term.as_ref().map(|t| t.mouse_tracking()).unwrap_or(false);
                match me.kind {
                    K::ScrollUp if !tracking => {
                        if let Some(t) = app.term.as_mut() {
                            t.scroll(3);
                        }
                    }
                    K::ScrollDown if !tracking => {
                        if let Some(t) = app.term.as_mut() {
                            t.scroll(-3);
                        }
                    }
                    _ => {
                        let col = me.column - px + 1;
                        let row = me.row - py + 1;
                        if let Some(t) = app.term.as_mut() {
                            t.send_mouse(&me, col, row);
                        }
                    }
                }
            } else if matches!(me.kind, K::Down(MouseButton::Left)) {
                // Click outside the pane (on the sidebar) returns focus there,
                // and selects the clicked row if there is one.
                app.blur_terminal();
                click_select(app, me.row);
            }
        }
        Mode::Browse => match me.kind {
            K::ScrollDown => app.move_selection(1),
            K::ScrollUp => app.move_selection(-1),
            K::Down(MouseButton::Left) => {
                if over_pane {
                    app.focus_terminal();
                } else {
                    handle_list_click(app, me.row);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// Resolve a click at screen row `y` to a list row. Selects a session (and
/// opens it on a double-click) or toggles a group header.
fn handle_list_click(app: &mut App, y: u16) {
    let hit = app
        .list_hits
        .borrow()
        .iter()
        .find(|(ry, h, _)| y >= *ry && y < ry.saturating_add(*h))
        .map(|(_, _, hit)| hit.clone());
    let Some(hit) = hit else { return };
    match hit {
        RowHit::Header(cwd) => {
            app.last_click = None;
            app.toggle_group(&cwd);
        }
        RowHit::Session(real_idx) => {
            let Some(pos) = app.select_by_real_index(real_idx) else {
                return;
            };
            // Double-click (same row, <400ms) opens the session.
            let now = std::time::Instant::now();
            let double = matches!(app.last_click, Some((p, t))
                if p == pos && now.duration_since(t) < std::time::Duration::from_millis(400));
            if double {
                app.last_click = None;
                if let Some(s) = app.selected_session() {
                    let id = s.id.clone();
                    let cwd = s.cwd.clone();
                    let is_alive = s.is_alive;
                    launcher::open_terminal_for(app, PendingExec::Resume { id, cwd, is_alive });
                }
            } else {
                app.last_click = Some((pos, now));
            }
        }
    }
}

/// Select (without opening) the session at screen row `y`, if any.
fn click_select(app: &mut App, y: u16) {
    let real = app
        .list_hits
        .borrow()
        .iter()
        .find(|(ry, h, _)| y >= *ry && y < ry.saturating_add(*h))
        .and_then(|(_, _, hit)| match hit {
            RowHit::Session(i) => Some(*i),
            RowHit::Header(_) => None,
        });
    if let Some(real_idx) = real {
        app.select_by_real_index(real_idx);
    }
}

fn handle_launch(app: &mut App, code: KeyCode) -> Option<ExitRequest> {
    // Borrow the form mutably via match-pattern.
    let form = match &mut app.mode {
        Mode::Launch(f) => f,
        _ => return None,
    };
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Browse;
        }
        KeyCode::Up => {
            if form.field > 0 {
                form.field -= 1;
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            form.field = (form.field + 1) % LaunchForm::FIELD_COUNT;
        }
        KeyCode::Char(' ') if matches!(form.field, 1..=4) => form.toggle_field(),
        KeyCode::Left if form.field == 0 => form.cycle_dir(false),
        KeyCode::Right if form.field == 0 => form.cycle_dir(true),
        KeyCode::Left | KeyCode::Right if form.field == 1 => form.toggle_field(),
        KeyCode::Char(c) if form.field == 0 => form.cwd.push(c),
        KeyCode::Backspace if form.field == 0 => {
            form.cwd.pop();
        }
        KeyCode::Char(c) if form.field == 5 => form.add_dir.push(c),
        KeyCode::Backspace if form.field == 5 => {
            form.add_dir.pop();
        }
        KeyCode::Enter => {
            let cwd = form.cwd.clone();
            let args = form.args();
            app.mode = Mode::Browse;
            launcher::open_terminal_for(app, PendingExec::Custom { cwd, args });
        }
        _ => {}
    }
    None
}

fn handle_browse(
    app: &mut App,
    code: KeyCode,
    _mods: KeyModifiers,
) -> Option<ExitRequest> {
    match code {
        KeyCode::Char('q') => return Some(ExitRequest::Quit),
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::PageUp => app.move_selection(-10),
        KeyCode::PageDown => app.move_selection(10),
        KeyCode::Char('g') => app.selected = 0,
        KeyCode::Char('G') => {
            let n = app.filtered_indices().len();
            if n > 0 {
                app.selected = n - 1;
            }
        }
        KeyCode::Enter => {
            if let Some(s) = app.selected_session() {
                let id = s.id.clone();
                let cwd = s.cwd.clone();
                let is_alive = s.is_alive;
                launcher::open_terminal_for(app, PendingExec::Resume { id, cwd, is_alive });
            }
        }
        KeyCode::Char('n') => {
            if let Some(s) = app.selected_session() {
                let cwd = s.cwd.clone();
                launcher::open_terminal_for(app, PendingExec::NewClaude { cwd });
            }
        }
        KeyCode::Char('N') => {
            // Open the launch options form for a brand new session.
            let cwd = app
                .selected_session()
                .map(|s| s.cwd.clone())
                .unwrap_or_else(|| {
                    dirs::home_dir()
                        .map(|h| h.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
            let dirs = app.recent_dirs();
            app.mode = Mode::Launch(LaunchForm::new(cwd, dirs));
        }
        KeyCode::Char('s') => {
            if let Some(s) = app.selected_session() {
                let cwd = s.cwd.clone();
                launcher::open_terminal_for(app, PendingExec::NewShell { cwd });
            }
        }
        // vim-style: move focus into the terminal pane (insert). No-op unless a
        // terminal is open.
        KeyCode::Char('i') | KeyCode::Char('l') => {
            if app.has_terminal() {
                app.focus_terminal();
            }
        }
        KeyCode::Char('/') => {
            app.filter.clear();
            app.mode = Mode::Filter;
        }
        KeyCode::Char('r') => {
            if let Some(s) = app.selected_session() {
                app.rename_buf = s.name.clone();
                app.mode = Mode::Rename;
            }
        }
        KeyCode::Char('R') => {
            app.kick_scan();
            app.flash("refreshing…");
        }
        KeyCode::Char(' ') | KeyCode::Tab => {
            app.toggle_group_of_selection();
        }
        KeyCode::Char('o') => {
            app.collapse_all_inactive();
            app.flash("collapsed inactive groups");
        }
        KeyCode::Char('O') => {
            app.collapsed_groups.clear();
            app.flash("expanded all groups");
        }
        KeyCode::Char('T') => {
            app.group_by_directory = !app.group_by_directory;
            app.clamp_selection();
            app.flash(if app.group_by_directory {
                "grouping by directory"
            } else {
                "flat list"
            });
        }
        KeyCode::Char('M') => {
            app.notifier.enabled = !app.notifier.enabled;
            app.flash(if app.notifier.enabled {
                "notifications on"
            } else {
                "notifications muted"
            });
        }
        KeyCode::Char('?') => {
            app.mode = Mode::Help;
        }
        KeyCode::Char(':') => {
            app.open_settings();
        }
        KeyCode::Char('c') => {
            app.mode = Mode::CostSummary;
        }
        KeyCode::Char('\\') => {
            // Prompt-less AI search: use current filter buffer as the query.
            if app.filter.is_empty() {
                app.flash("type / first, then \\ to run AI search");
            } else {
                let q = app.filter.clone();
                app.kick_ai_search(q);
                app.flash("AI searching…");
            }
        }
        KeyCode::Char('A') => {
            app.kick_auto_name();
            if app.auto_naming {
                app.flash(format!(
                    "auto-naming {} session(s)…",
                    app.auto_name_progress.1
                ));
            }
        }
        KeyCode::Char('D') => {
            app.mode = Mode::Confirm(ConfirmAction::DeleteJunk);
        }
        KeyCode::Char('E') => {
            app.mode = Mode::Confirm(ConfirmAction::DeleteEmpty);
        }
        KeyCode::Char('K') => {
            app.ask_kill_tmux();
        }
        _ => {}
    }
    None
}

fn handle_filter(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.filter.clear();
            app.mode = Mode::Browse;
            app.clamp_selection();
        }
        KeyCode::Enter => {
            // Literal-first; if no match, fall back to AI search.
            let literal = app.filtered_indices().len();
            if literal == 0 && !app.filter.is_empty() {
                let q = app.filter.clone();
                app.kick_ai_search(q);
                app.flash("no literal match — AI searching…");
            }
            app.mode = Mode::Browse;
            app.clamp_selection();
        }
        KeyCode::Backspace => {
            app.filter.pop();
            app.clamp_selection();
        }
        KeyCode::Char(c) => {
            app.filter.push(c);
            app.selected = 0;
        }
        _ => {}
    }
}

fn handle_rename(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.rename_buf.clear();
            app.mode = Mode::Browse;
        }
        KeyCode::Enter => {
            app.rename_selected();
            app.mode = Mode::Browse;
            app.flash("renamed");
        }
        KeyCode::Backspace => {
            app.rename_buf.pop();
        }
        KeyCode::Char(c) => {
            app.rename_buf.push(c);
        }
        _ => {}
    }
}

fn handle_confirm(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.perform_confirm();
        }
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            app.mode = Mode::Browse;
        }
        _ => {}
    }
}

fn handle_settings(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.settings_input.clear();
            app.settings_budget_input.clear();
            app.mode = Mode::Browse;
        }
        KeyCode::Up => {
            if app.settings_field > 0 {
                app.settings_field -= 1;
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            app.settings_field = (app.settings_field + 1) % 2;
        }
        KeyCode::Enter => {
            // Validate the prefix.
            let spec = match config::KeySpec::parse(&app.settings_input) {
                Ok(s) if s.is_reserved() => {
                    app.flash("that key is reserved (Ctrl-C / Ctrl-D)");
                    return;
                }
                Ok(s) => s,
                Err(e) => {
                    app.flash(format!("invalid key: {e}"));
                    return;
                }
            };
            // Validate the daily budget (empty = off).
            let budget = {
                let t = app.settings_budget_input.trim();
                if t.is_empty() {
                    None
                } else {
                    match t.parse::<f64>() {
                        Ok(v) if v > 0.0 => Some(v),
                        Ok(_) => None,
                        Err(_) => {
                            app.flash("invalid budget (use a number like 25)");
                            return;
                        }
                    }
                }
            };
            app.config.escape_prefix = spec;
            app.config.daily_budget_usd = budget;
            match config::save(&app.config) {
                Ok(()) => app.flash("settings saved"),
                Err(e) => app.flash(format!("save failed: {e}")),
            }
            app.settings_input.clear();
            app.settings_budget_input.clear();
            app.mode = Mode::Browse;
        }
        KeyCode::Backspace => {
            if app.settings_field == 0 {
                app.settings_input.pop();
            } else {
                app.settings_budget_input.pop();
            }
        }
        KeyCode::Char(c) => {
            if app.settings_field == 0 {
                if app.settings_input.chars().count() < 24 {
                    app.settings_input.push(c);
                }
            } else if (c.is_ascii_digit() || c == '.')
                && app.settings_budget_input.chars().count() < 12
            {
                app.settings_budget_input.push(c);
            }
        }
        _ => {}
    }
}
