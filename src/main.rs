mod ai;
mod app;
mod models;
mod notifications;
mod scanner;
mod tmux;
mod ui;
mod watcher;

use std::io::{self, Write};
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, ConfirmAction, LaunchForm, Mode, PendingExec};

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
                println!("minionscode {}", env!("CARGO_PKG_VERSION"));
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
        "MinionsCode — TUI for Claude Code sessions

USAGE:
    minionscode [OPTIONS]

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
    let mut app = App::new(args.history_days);

    loop {
        enter_tui()?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;

        let exit_request = run_loop(&mut terminal, &mut app)?;
        leave_tui(&mut terminal)?;

        match exit_request {
            ExitRequest::Quit => return Ok(()),
            ExitRequest::Exec(pending) => {
                run_exec(pending);
                // After the child exits, return to the TUI and trigger a fresh scan.
                app.kick_scan();
                continue;
            }
        }
    }
}

enum ExitRequest {
    Quit,
    Exec(PendingExec),
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

fn enter_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(())
}

fn leave_tui<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<ExitRequest> {
    let tick = Duration::from_millis(120);
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        app.tick();

        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if let Some(req) = handle_key(app, key.code, key.modifiers) {
                    return Ok(req);
                }
            }
        }
    }
}

fn handle_key(
    app: &mut App,
    code: KeyCode,
    mods: KeyModifiers,
) -> Option<ExitRequest> {
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
        KeyCode::Char(' ') if form.field != 4 => form.toggle_field(),
        KeyCode::Left | KeyCode::Right if form.field == 0 => form.toggle_field(),
        KeyCode::Char(c) if form.field == 4 => form.add_dir.push(c),
        KeyCode::Backspace if form.field == 4 => {
            form.add_dir.pop();
        }
        KeyCode::Enter => {
            let cwd = form.cwd.clone();
            let args = form.args();
            app.mode = Mode::Browse;
            return Some(ExitRequest::Exec(PendingExec::Custom { cwd, args }));
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
                return Some(ExitRequest::Exec(PendingExec::Resume { id, cwd }));
            }
        }
        KeyCode::Char('n') => {
            if let Some(s) = app.selected_session() {
                let cwd = s.cwd.clone();
                return Some(ExitRequest::Exec(PendingExec::NewClaude { cwd }));
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
            app.mode = Mode::Launch(LaunchForm::new(cwd, None));
        }
        KeyCode::Char('s') => {
            if let Some(s) = app.selected_session() {
                let cwd = s.cwd.clone();
                return Some(ExitRequest::Exec(PendingExec::NewShell { cwd }));
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

fn run_exec(pending: PendingExec) {
    let _ = io::stdout().flush();
    // tmux mode: only when tmux is installed AND we're not already inside tmux
    // (we don't try to nest — nested tmux sessions are a UX trap).
    let use_tmux = tmux::available() && !tmux::inside_tmux();
    if use_tmux {
        run_exec_tmux(pending);
    } else {
        run_exec_direct(pending);
    }
}

fn run_exec_tmux(pending: PendingExec) {
    match pending {
        PendingExec::Resume { id, cwd } => {
            let Some(claude) = find_claude_binary() else {
                eprintln!("claude binary not found");
                pause();
                return;
            };
            let name = tmux::resume_name(&id);
            let cmd = tmux::join_command(&[&claude, "--resume", &id]);
            tmux::ensure_session(&name, &cwd, &cmd);
            print_attach_hint(&name);
            tmux::attach(&name);
        }
        PendingExec::NewClaude { cwd } => {
            let Some(claude) = find_claude_binary() else {
                eprintln!("claude binary not found");
                pause();
                return;
            };
            let name = tmux::new_claude_name();
            let cmd = tmux::sh_quote(&claude);
            tmux::ensure_session(&name, &cwd, &cmd);
            print_attach_hint(&name);
            tmux::attach(&name);
        }
        PendingExec::NewShell { cwd } => {
            let name = tmux::new_shell_name();
            tmux::ensure_session_shell(&name, &cwd);
            print_attach_hint(&name);
            tmux::attach(&name);
        }
        PendingExec::Custom { cwd, args } => {
            let Some(claude) = find_claude_binary() else {
                eprintln!("claude binary not found");
                pause();
                return;
            };
            let name = tmux::new_claude_name();
            let mut parts: Vec<&str> = Vec::with_capacity(1 + args.len());
            parts.push(&claude);
            for a in &args {
                parts.push(a);
            }
            let cmd = tmux::join_command(&parts);
            tmux::ensure_session(&name, &cwd, &cmd);
            print_attach_hint(&name);
            tmux::attach(&name);
        }
    }
}

fn print_attach_hint(name: &str) {
    // One-line hint before tmux takes the screen — visible for a moment so
    // first-time users learn how to detach. tmux's own banner clears it.
    let _ = writeln!(
        io::stdout(),
        "→ tmux session {}  (Ctrl-b d to detach back to MinionsCode)",
        name
    );
    let _ = io::stdout().flush();
}

fn run_exec_direct(pending: PendingExec) {
    match pending {
        PendingExec::Resume { id, cwd } => {
            if let Some(claude) = find_claude_binary() {
                let mut cmd = Command::new(claude);
                cmd.arg("--resume").arg(id);
                cmd.current_dir(&cwd);
                let _ = cmd.status();
            } else {
                eprintln!("claude binary not found in PATH or known locations");
                pause();
            }
        }
        PendingExec::NewClaude { cwd } => {
            if let Some(claude) = find_claude_binary() {
                let mut cmd = Command::new(claude);
                cmd.current_dir(&cwd);
                let _ = cmd.status();
            } else {
                eprintln!("claude binary not found in PATH or known locations");
                pause();
            }
        }
        PendingExec::NewShell { cwd } => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let mut cmd = Command::new(shell);
            cmd.arg("-l");
            cmd.current_dir(&cwd);
            let _ = cmd.status();
        }
        PendingExec::Custom { cwd, args } => {
            if let Some(claude) = find_claude_binary() {
                let mut cmd = Command::new(claude);
                cmd.args(args);
                cmd.current_dir(&cwd);
                let _ = cmd.status();
            } else {
                eprintln!("claude binary not found");
                pause();
            }
        }
    }
}

fn pause() {
    eprint!("\npress enter to return to MinionsCode…");
    let _ = io::stderr().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}
