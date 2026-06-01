mod ai;
mod app;
mod config;
mod models;
mod notifications;
mod pty;
mod scanner;
mod tmux;
mod ui;
mod watcher;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, ConfirmAction, LaunchForm, Mode, PendingExec};
use crate::pty::{TermSession, TerminalSpec};

fn parse_args() -> Args {
    let mut a = Args::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--days" | "-d" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    a.history_days = v;
                }
            }
            "--list" | "-l" => {
                a.list_only = true;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("managecode {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            _ => {}
        }
    }
    a
}

#[derive(Clone, Copy)]
struct Args {
    history_days: i64,
    list_only: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            history_days: 30,
            list_only: false,
        }
    }
}

fn print_help() {
    println!(
        "ManageCode — TUI for Claude Code sessions

USAGE:
    managecode [OPTIONS]

OPTIONS:
    -d, --days <N>     History horizon in days (default 30)
    -l, --list         Print sessions and exit (non-interactive)
    -h, --help         Show this help
    -V, --version      Show version

KEYS (inside the TUI):
    ↑↓ / jk     navigate
    ⏎           resume selected session  (exec claude --resume)
    n           new claude in selected cwd
    s           new shell in selected cwd
    /           filter
    r           rename
    R           refresh now
    ?           help
    q / ctrl-c  quit"
    );
}

fn main() -> Result<()> {
    let args = parse_args();
    if args.list_only {
        return run_list(args.history_days);
    }
    install_panic_hook();
    let mut app = App::new(args.history_days);

    loop {
        enter_tui()?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;

        let exit_request = run_loop(&mut terminal, &mut app)?;
        leave_tui(&mut terminal)?;

        match exit_request {
            ExitRequest::Quit => return Ok(()),
        }
    }
}

enum ExitRequest {
    Quit,
}

fn run_list(history_days: i64) -> Result<()> {
    let sessions = scanner::scan(history_days);
    let total: f64 = sessions.iter().map(|s| s.cost).sum();
    let active = sessions.iter().filter(|s| s.is_alive).count();
    println!(
        "{} sessions  ·  {} active  ·  ${:.4} total\n",
        sessions.len(),
        active,
        total
    );
    for s in &sessions {
        let mark = if s.is_alive { "●" } else { "○" };
        let model = models::model_short(s.model.as_deref());
        println!(
            "{} {:<8}  {:>10}  ${:>8.4}  {}  {}",
            mark,
            model,
            format_count(s.usage.total_input + s.usage.cache_read + s.usage.cache_creation + s.usage.total_output),
            s.cost,
            truncate_str(&s.name, 30),
            models::short_path(&s.cwd),
        );
    }
    Ok(())
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Restore the terminal (raw mode, alternate screen, mouse capture) if we panic
/// mid-render, so the user's shell isn't left in a broken state.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original(info);
    }));
}

fn enter_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(())
}

fn leave_tui<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<()>
where
    <B as ratatui::backend::Backend>::Error: std::error::Error + Send + Sync + 'static,
{
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<ExitRequest>
where
    <B as ratatui::backend::Backend>::Error: std::error::Error + Send + Sync + 'static,
{
    loop {
        // Keep the embedded terminal sized to its pane and alive. The pane size
        // is whatever the renderer measured last frame (see app.term_size).
        if app.has_terminal() {
            let (_x, _y, cols, rows) = app.term_area.get();
            service_terminal(app, rows, cols);
        }

        terminal.draw(|f| ui::draw(f, app))?;
        app.tick();

        // The embedded terminal updates asynchronously, so poll faster while it
        // is on screen for a responsive feel.
        let tick = if app.has_terminal() {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(120)
        };

        if event::poll(tick)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if let Some(req) = handle_key(app, key.code, key.modifiers) {
                        return Ok(req);
                    }
                }
                Event::Mouse(me) => handle_mouse(app, me),
                _ => {}
            }
        }
    }
}

/// Spawn a queued terminal once the pane size is known, keep it resized, and
/// drop it when the child exits.
fn service_terminal(app: &mut App, rows: u16, cols: u16) {
    if app.term.is_none() {
        if let Some(spec) = app.pending_terminal.take() {
            let cmd = spec.build_command();
            match TermSession::spawn(cmd, rows, cols, spec.title.clone()) {
                Ok(t) => app.term = Some(t),
                Err(e) => {
                    app.flash(format!("terminal failed: {e}"));
                    app.close_terminal();
                    return;
                }
            }
        }
    }
    if let Some(t) = app.term.as_mut() {
        t.resize(rows, cols);
        if !t.is_alive() {
            app.close_terminal();
            app.kick_scan();
        }
    }
}

fn handle_key(
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
        t.send_key(code, mods);
    }
}

/// Mouse routing: forward to the embedded terminal when it's focused and the
/// pointer is over the pane; in the sidebar, the wheel moves the selection and
/// a click on the terminal pane focuses it.
fn handle_mouse(app: &mut App, me: crossterm::event::MouseEvent) {
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
                let col = me.column - px + 1;
                let row = me.row - py + 1;
                if let Some(t) = app.term.as_mut() {
                    t.send_mouse(&me, col, row);
                }
            }
        }
        Mode::Browse => match me.kind {
            K::ScrollDown => app.move_selection(1),
            K::ScrollUp => app.move_selection(-1),
            K::Down(MouseButton::Left) if over_pane => app.focus_terminal(),
            _ => {}
        },
        _ => {}
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
        KeyCode::Char(' ') if matches!(form.field, 1 | 2 | 3 | 4) => form.toggle_field(),
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
            open_terminal_for(app, PendingExec::Custom { cwd, args });
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
                open_terminal_for(app, PendingExec::Resume { id, cwd });
            }
        }
        KeyCode::Char('n') => {
            if let Some(s) = app.selected_session() {
                let cwd = s.cwd.clone();
                open_terminal_for(app, PendingExec::NewClaude { cwd });
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
            app.mode = Mode::Launch(LaunchForm::new(cwd, None, dirs));
        }
        KeyCode::Char('s') => {
            if let Some(s) = app.selected_session() {
                let cwd = s.cwd.clone();
                open_terminal_for(app, PendingExec::NewShell { cwd });
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

fn find_claude_binary() -> Option<String> {
    if let Ok(p) = std::env::var("CLAUDE_BIN") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    let mut candidates: Vec<String> = vec![
        "/opt/homebrew/bin/claude".into(),
        "/usr/local/bin/claude".into(),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".claude/local/bin/claude").to_string_lossy().to_string());
        candidates.push(home.join(".local/bin/claude").to_string_lossy().to_string());
    }
    for c in candidates {
        if std::fs::metadata(&c).is_ok() {
            return Some(c);
        }
    }
    // Fall back to PATH lookup.
    Some("claude".into())
}

/// Open an embedded terminal for a launch intent. When tmux is available the
/// command runs inside a detached tmux session that we `tmux attach` to in the
/// PTY (so it persists across detach/quit); otherwise the command runs directly
/// in the PTY.
fn open_terminal_for(app: &mut App, pending: PendingExec) {
    let use_tmux = tmux::available() && !tmux::inside_tmux();
    let claude = find_claude_binary();
    let spec = match pending {
        PendingExec::Resume { id, cwd } => {
            let Some(claude) = claude else {
                app.flash("claude binary not found");
                return;
            };
            if use_tmux {
                let name = tmux::resume_name(&id);
                let cmd = tmux::join_command(&[&claude, "--resume", &id]);
                tmux::ensure_session(&name, &cwd, &cmd);
                attach_spec(&name, &cwd, "claude")
            } else {
                TerminalSpec {
                    cwd,
                    argv: vec![claude, "--resume".into(), id],
                    title: "claude".into(),
                }
            }
        }
        PendingExec::NewClaude { cwd } => {
            let Some(claude) = claude else {
                app.flash("claude binary not found");
                return;
            };
            if use_tmux {
                let name = tmux::new_claude_name();
                let cmd = tmux::sh_quote(&claude);
                tmux::ensure_session(&name, &cwd, &cmd);
                attach_spec(&name, &cwd, "claude")
            } else {
                TerminalSpec {
                    cwd,
                    argv: vec![claude],
                    title: "claude".into(),
                }
            }
        }
        PendingExec::NewShell { cwd } => {
            if use_tmux {
                let name = tmux::new_shell_name();
                tmux::ensure_session_shell(&name, &cwd);
                attach_spec(&name, &cwd, "shell")
            } else {
                TerminalSpec {
                    cwd,
                    argv: vec![],
                    title: "shell".into(),
                }
            }
        }
        PendingExec::Custom { cwd, args } => {
            let Some(claude) = claude else {
                app.flash("claude binary not found");
                return;
            };
            if use_tmux {
                let name = tmux::new_claude_name();
                let mut parts: Vec<&str> = Vec::with_capacity(1 + args.len());
                parts.push(&claude);
                for a in &args {
                    parts.push(a);
                }
                let cmd = tmux::join_command(&parts);
                tmux::ensure_session(&name, &cwd, &cmd);
                attach_spec(&name, &cwd, "claude")
            } else {
                let mut argv = vec![claude];
                argv.extend(args);
                TerminalSpec {
                    cwd,
                    argv,
                    title: "claude".into(),
                }
            }
        }
    };
    app.request_terminal(spec);
}

/// A TerminalSpec that attaches to an existing tmux session inside the PTY.
fn attach_spec(name: &str, cwd: &str, title: &str) -> TerminalSpec {
    TerminalSpec {
        cwd: cwd.to_string(),
        argv: vec![
            "tmux".into(),
            "attach-session".into(),
            "-t".into(),
            name.to_string(),
        ],
        title: title.to_string(),
    }
}
