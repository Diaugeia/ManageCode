mod ai;
mod app;
mod config;
mod input;
mod launcher;
mod models;
mod notifications;
mod pty;
mod scanner;
mod tmux;
mod ui;
mod update;
mod watcher;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::App;
use crate::pty::TermSession;

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
            "--update" | "-u" => {
                a.update = true;
            }
            "--no-update-check" => {
                a.no_update_check = true;
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
    update: bool,
    no_update_check: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            history_days: 30,
            list_only: false,
            update: false,
            no_update_check: false,
        }
    }
}

fn print_help() {
    println!(
        "ManageCode — TUI for Claude Code sessions

USAGE:
    managecode [OPTIONS]

OPTIONS:
    -d, --days <N>       History horizon in days (default 30)
    -l, --list           Print sessions and exit (non-interactive)
    -u, --update         Update to the latest release and exit
        --no-update-check  Skip the startup check for a newer release
    -h, --help           Show this help
    -V, --version        Show version

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
    if args.update {
        let status = update::run_update()?;
        std::process::exit(status.code().unwrap_or(1));
    }
    if args.list_only {
        return run_list(args.history_days);
    }
    install_panic_hook();
    // Check for a newer release in the background unless opted out.
    let check_updates = !args.no_update_check && !update::check_disabled();
    let mut app = App::new(args.history_days, check_updates);

    enter_tui()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let ExitRequest::Quit = run_loop(&mut terminal, &mut app)?;
    leave_tui(&mut terminal)?;
    Ok(())
}

pub enum ExitRequest {
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
            format_count(s.usage.total_input + s.usage.cache_read + s.usage.cache_creation() + s.usage.total_output),
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
                    if let Some(req) = input::handle_key(app, key.code, key.modifiers) {
                        return Ok(req);
                    }
                }
                Event::Mouse(me) => input::handle_mouse(app, me),
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


