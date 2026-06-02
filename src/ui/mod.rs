use chrono::Local;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Padding, Paragraph, Wrap},
    Frame,
};
use tui_term::widget::PseudoTerminal;

use crate::app::{App, LaunchForm, Mode, Row, RowHit, SettingsForm};
use crate::models::{model_short, short_path, SessionInfo};

// Terminal-native palette. Backgrounds inherit the terminal (`Color::Reset`) so
// every panel shows through to the user's theme — no hardcoded scheme and no
// seam against the embedded terminal pane. Accents and status colors use the 16
// ANSI colors so they track whatever palette the terminal defines.
const ACCENT: Color = Color::Cyan; // titles, borders, selection (was gold)
const ACCENT_DIM: Color = Color::Blue; // unfocused borders / secondary accent
const BG: Color = Color::Reset; // viewport / header / footer background
const PANEL: Color = Color::Reset; // bordered-panel background
const SEL_FG: Color = Color::Black; // foreground on a selected (ACCENT) row
const TEXT: Color = Color::Reset; // primary text (terminal default fg)
const MUTED: Color = Color::DarkGray;
const LIVE: Color = Color::Green;
const WARN: Color = Color::Yellow;
const RED: Color = Color::Red;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Three layout tiers based on width:
/// - wide   (>=110): two-pane list + detail side-by-side
/// - medium (70..110, h>=24): list on top, detail stacked below
/// - narrow (<70 or short): list only; selected session's key info collapses into footer
#[derive(Clone, Copy, PartialEq)]
enum Layoutness {
    Wide,
    Stacked,
    Narrow,
}

fn pick_layout(area: Rect) -> Layoutness {
    if area.width >= 110 && area.height >= 20 {
        Layoutness::Wide
    } else if area.width >= 70 && area.height >= 24 {
        Layoutness::Stacked
    } else {
        Layoutness::Narrow
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    // Outer fill so the whole viewport gets the background tone, not just inside borders.
    f.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let tier = pick_layout(area);
    let footer_height = if matches!(tier, Layoutness::Narrow) {
        3
    } else {
        2
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(footer_height),
        ])
        .split(area);

    draw_header(f, layout[0], app, tier);

    // Build the visible rows once per frame and share them with the list,
    // detail, and footer (each used to rebuild them independently).
    let rows = app.visible_rows();

    if app.has_terminal() {
        // Sidebar (session list) + live embedded terminal.
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(32), Constraint::Min(40)])
            .split(layout[1]);
        let term_focused = matches!(app.mode, Mode::Terminal);
        draw_session_list(f, body[0], app, Layoutness::Narrow, &rows);
        draw_terminal_pane(f, body[1], app, term_focused);
    } else {
        match tier {
            Layoutness::Wide => {
                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(45), Constraint::Min(40)])
                    .split(layout[1]);
                draw_session_list(f, body[0], app, tier, &rows);
                draw_detail(f, body[1], app, tier, &rows);
            }
            Layoutness::Stacked => {
                let body = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(60), Constraint::Min(8)])
                    .split(layout[1]);
                draw_session_list(f, body[0], app, tier, &rows);
                draw_detail(f, body[1], app, tier, &rows);
            }
            Layoutness::Narrow => {
                draw_session_list(f, layout[1], app, tier, &rows);
            }
        }
    }

    draw_footer(f, layout[2], app, tier, &rows);

    // Modal overlays.
    match &app.mode {
        Mode::Filter => draw_filter_overlay(f, area, app),
        Mode::Rename { buf } => draw_rename_overlay(f, area, buf),
        Mode::Help => draw_help_overlay(f, area, app),
        Mode::Confirm(_) => draw_confirm_overlay(f, area, app),
        Mode::Launch(form) => draw_launch_overlay(f, area, form),
        Mode::Settings(form) => draw_settings_overlay(f, area, form),
        Mode::CostSummary => draw_cost_summary_overlay(f, area, app),
        Mode::MigrateMemory { src, input } => draw_migrate_overlay(f, area, src, input),
        Mode::TreeRoot { input } => draw_tree_root_overlay(f, area, input),
        Mode::Browse => {}
        // Handled inline by the sidebar+terminal layout; no modal overlay.
        Mode::Terminal => {}
    }

    if let Some((msg, _)) = &app.message {
        draw_toast(f, area, msg);
    }
}

fn panel_block(title: &str, focused: bool) -> Block<'_> {
    let border_color = if focused { ACCENT } else { ACCENT_DIM };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
}

/// The "  ·  " dot separator used between header / footer segments.
fn sep(color: Color) -> Span<'static> {
    Span::styled("  ·  ", Style::default().fg(color))
}

/// The session under a given row index, or `None` if it's a header / tree node.
fn session_at<'a>(app: &'a App, rows: &[Row], selected: usize) -> Option<&'a SessionInfo> {
    match rows.get(selected) {
        Some(Row::Session { idx, .. }) => app.sessions.get(*idx),
        _ => None,
    }
}

/// The selection-highlight style (foreground on the accent bar), bold or not.
fn sel_style(bold: bool) -> Style {
    let s = Style::default().fg(SEL_FG).bg(ACCENT);
    if bold {
        s.add_modifier(Modifier::BOLD)
    } else {
        s
    }
}

/// Trailing `●{alive}  {total}` count spans shared by group headers and tree
/// nodes.
fn count_spans(alive: usize, total: usize) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw("  ")];
    if alive > 0 {
        spans.push(Span::styled(
            format!("●{}", alive),
            Style::default().fg(LIVE),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        format!("{}", total),
        Style::default().fg(MUTED),
    ));
    spans
}

/// Format a single USD amount for display. Uses 2 decimals (so amounts line up
/// and read cleanly), but keeps 4 decimals for small non-zero amounts under
/// $0.10 so sub-cent costs don't collapse to `$0.00`.
fn fmt_usd(v: f64) -> String {
    if v > 0.0 && v < 0.10 {
        format!("${:.4}", v)
    } else {
        format!("${:.2}", v)
    }
}

fn draw_terminal_pane(f: &mut Frame, area: Rect, app: &App, focused: bool) {
    let title = app
        .term
        .as_ref()
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "terminal".into());
    let block = panel_block(&title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Report the pane's content size so the run loop can resize the PTY to match.
    app.term_area
        .set((inner.x, inner.y, inner.width.max(1), inner.height.max(1)));

    match &app.term {
        Some(t) => {
            let screen = t.screen();
            f.render_widget(PseudoTerminal::new(&screen), inner);
        }
        None => {
            let p = Paragraph::new(Span::styled("starting…", Style::default().fg(MUTED)))
                .alignment(Alignment::Center);
            f.render_widget(p, inner);
        }
    }
}

fn draw_terminal_footer(f: &mut Frame, area: Rect, app: &App) {
    let prefix = app.config.escape_prefix.label();
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_DIM))
        .style(Style::default().bg(BG));
    f.render_widget(block, area);
    let line = Line::from(vec![
        Span::styled(
            prefix,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" focus list", Style::default().fg(MUTED)),
        sep(ACCENT_DIM),
        Span::styled(
            "keys",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" → terminal", Style::default().fg(MUTED)),
    ]);
    f.render_widget(
        Paragraph::new(line),
        Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: 1,
        },
    );
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App, tier: Layoutness, rows: &[Row]) {
    // Terminal pane focused: a dedicated footer shows the configured prefix.
    if matches!(app.mode, Mode::Terminal) {
        draw_terminal_footer(f, area, app);
        return;
    }
    let narrow = matches!(tier, Layoutness::Narrow);
    let owned = |v: Vec<(&str, &str)>| -> Vec<(String, String)> {
        v.into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    };
    // Browse hints are generated from the central keymap so they never drift
    // from the actual bindings; other modes are fixed.
    let mut hints: Vec<(String, String)> = match app.mode {
        Mode::Browse => app.keymap.footer_hints(narrow),
        Mode::Filter => owned(vec![("⏎", "apply"), ("\\", "AI search"), ("esc", "cancel")]),
        Mode::Rename { .. } => owned(vec![("⏎", "save"), ("esc", "cancel")]),
        Mode::MigrateMemory { .. } => owned(vec![
            ("⏎", "migrate"),
            ("←→", "recent dir"),
            ("esc", "cancel"),
        ]),
        Mode::TreeRoot { .. } => owned(vec![("⏎", "set root"), ("←→", "dir"), ("esc", "cancel")]),
        Mode::Help | Mode::Confirm(_) => owned(vec![("esc", "close")]),
        Mode::Launch(_) => owned(vec![
            ("⏎", "launch"),
            ("space", "toggle"),
            ("esc", "cancel"),
        ]),
        Mode::Settings(_) => owned(vec![("⏎", "save"), ("esc", "cancel")]),
        Mode::CostSummary => owned(vec![("esc", "close")]),
        // Terminal footer is drawn separately (shows the configured prefix).
        Mode::Terminal => vec![],
    };

    // When a terminal is open but the sidebar is focused, advertise how to jump in.
    if matches!(app.mode, Mode::Browse) && app.has_terminal() {
        hints.insert(0, ("i".to_string(), "terminal".to_string()));
    }

    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, (k, v)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(ACCENT_DIM)));
        }
        spans.push(Span::styled(
            (*k).to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled((*v).to_string(), Style::default().fg(MUTED)));
    }
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_DIM))
        .style(Style::default().bg(BG));
    f.render_widget(block, area);

    if narrow {
        // Two-line footer: selection summary + key hints.
        if let Some(s) = session_at(app, rows, app.selected) {
            let summary = Line::from(vec![
                Span::styled(
                    model_short(s.model.as_deref()).to_string(),
                    Style::default().fg(ACCENT).bold(),
                ),
                Span::raw("  "),
                Span::styled(fmt_usd(s.cost), Style::default().fg(TEXT)),
                sep(ACCENT_DIM),
                Span::styled(
                    truncate(&short_path(&s.cwd), area.width.saturating_sub(20) as usize),
                    Style::default().fg(MUTED),
                ),
            ]);
            f.render_widget(
                Paragraph::new(summary),
                Rect {
                    x: area.x + 1,
                    y: area.y + 1,
                    width: area.width.saturating_sub(2),
                    height: 1,
                },
            );
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect {
                x: area.x + 1,
                y: area.y + 2,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    } else {
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    }
}

mod detail;
mod header;
mod list;
mod overlays;
use detail::{draw_detail, truncate};
use header::draw_header;
use list::draw_session_list;
use overlays::*;
