use chrono::Local;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Padding, Paragraph, Wrap},
    Frame,
};
use tui_term::widget::PseudoTerminal;

use crate::app::{App, LaunchForm, Mode, Row, RowHit};
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

    if app.has_terminal() {
        // Sidebar (session list) + live embedded terminal.
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(32), Constraint::Min(40)])
            .split(layout[1]);
        let term_focused = matches!(app.mode, Mode::Terminal);
        draw_session_list(f, body[0], app, Layoutness::Narrow);
        draw_terminal_pane(f, body[1], app, term_focused);
    } else {
        match tier {
            Layoutness::Wide => {
                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(45), Constraint::Min(40)])
                    .split(layout[1]);
                draw_session_list(f, body[0], app, tier);
                draw_detail(f, body[1], app, tier);
            }
            Layoutness::Stacked => {
                let body = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(60), Constraint::Min(8)])
                    .split(layout[1]);
                draw_session_list(f, body[0], app, tier);
                draw_detail(f, body[1], app, tier);
            }
            Layoutness::Narrow => {
                draw_session_list(f, layout[1], app, tier);
            }
        }
    }

    draw_footer(f, layout[2], app, tier);

    // Modal overlays.
    match &app.mode {
        Mode::Filter => draw_filter_overlay(f, area, app),
        Mode::Rename => draw_rename_overlay(f, area, app),
        Mode::Help => draw_help_overlay(f, area),
        Mode::Confirm(_) => draw_confirm_overlay(f, area, app),
        Mode::Launch(form) => draw_launch_overlay(f, area, form),
        Mode::Settings => draw_settings_overlay(f, area, app),
        Mode::CostSummary => draw_cost_summary_overlay(f, area, app),
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

fn draw_header(f: &mut Frame, area: Rect, app: &App, tier: Layoutness) {
    let spin = SPINNER[app.spinner_phase % SPINNER.len()];
    let total = app.total_cost();
    let active = app.active_count();
    let count = app.sessions.len();

    let title = if matches!(tier, Layoutness::Narrow) {
        Line::from(vec![
            Span::styled("◆ ", Style::default().fg(ACCENT)),
            Span::styled(
                "ManageCode",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("◆ ", Style::default().fg(ACCENT)),
            Span::styled(
                "ManageCode",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Claude session dashboard", Style::default().fg(MUTED)),
        ])
    };

    let busy = app.scanning || app.ai_running || app.auto_naming;
    let busy_label: String = if app.auto_naming {
        format!(
            "naming {}/{}",
            app.auto_name_progress.0, app.auto_name_progress.1
        )
    } else if app.ai_running {
        "AI search".to_string()
    } else if app.scanning {
        "scanning".to_string()
    } else {
        String::new()
    };

    let stats = if matches!(tier, Layoutness::Narrow) {
        Line::from(vec![
            Span::styled(
                if busy { spin } else { "●" },
                Style::default().fg(if busy { WARN } else { LIVE }),
            ),
            Span::raw(" "),
            Span::styled(
                format!("${:.2}", total),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ])
        .alignment(Alignment::Right)
    } else {
        let mut spans = vec![
            Span::styled(
                if busy { spin } else { "●" },
                Style::default().fg(if busy { WARN } else { LIVE }),
            ),
            Span::raw("  "),
        ];
        if !busy_label.is_empty() {
            spans.push(Span::styled(
                busy_label.clone(),
                Style::default().fg(WARN),
            ));
            spans.push(sep(MUTED));
        }
        spans.push(Span::styled(
            format!("{} active", active),
            Style::default().fg(LIVE),
        ));
        let tmux_n = app.tmux_count();
        if tmux_n > 0 {
            spans.push(sep(MUTED));
            spans.push(Span::styled(
                format!("▶ {} tmux", tmux_n),
                Style::default().fg(Color::Rgb(0x6F, 0xD9, 0xCB)),
            ));
        }
        spans.push(sep(MUTED));
        spans.push(Span::styled(
            format!("{} total", count),
            Style::default().fg(TEXT),
        ));
        spans.push(sep(MUTED));
        spans.push(Span::styled(
            format!("${:.2}", total),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        // Today's spend, tinted by how close it is to the daily budget.
        let today = app.today_cost();
        if today > 0.0 || app.config.daily_budget_usd.is_some() {
            spans.push(sep(MUTED));
            let (txt, color) = match app.config.daily_budget_usd {
                Some(limit) if limit > 0.0 => {
                    let c = if today >= limit {
                        RED
                    } else if today >= limit * 0.8 {
                        WARN
                    } else {
                        LIVE
                    };
                    (format!("today ${:.2}/{:.0}", today, limit), c)
                }
                _ => (format!("today ${:.2}", today), LIVE),
            };
            spans.push(Span::styled(txt, Style::default().fg(color).add_modifier(Modifier::BOLD)));
        }
        if !app.notifier.enabled {
            spans.push(sep(MUTED));
            spans.push(Span::styled("🔕 muted", Style::default().fg(MUTED)));
        }
        if let Some(tag) = &app.update_available {
            spans.push(sep(MUTED));
            spans.push(Span::styled(
                format!("⬆ {tag} — managecode --update"),
                Style::default().fg(LIVE).add_modifier(Modifier::BOLD),
            ));
        }
        Line::from(spans).alignment(Alignment::Right)
    };

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(ACCENT_DIM))
        .style(Style::default().bg(BG));
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: 1,
        });

    f.render_widget(Paragraph::new(title), cols[0]);
    f.render_widget(Paragraph::new(stats), cols[1]);
}

fn draw_session_list(f: &mut Frame, area: Rect, app: &App, tier: Layoutness) {
    let block = panel_block("Sessions", matches!(app.mode, Mode::Browse | Mode::Filter));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = app.visible_rows();
    // Reset the click hit-map; it is rebuilt as rows are drawn below.
    app.list_hits.borrow_mut().clear();
    if rows.is_empty() {
        let msg = if app.scanning {
            "scanning ~/.claude…"
        } else {
            "no sessions match"
        };
        let p = Paragraph::new(Span::styled(msg, Style::default().fg(MUTED)))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let avail = inner.height as usize;
    if avail == 0 {
        return;
    }
    let session_height: usize = if matches!(tier, Layoutness::Narrow) { 1 } else { 2 };

    // Walk rows once to compute heights, then pick a viewport that keeps the
    // currently selected session row in view.
    let row_heights: Vec<usize> = rows
        .iter()
        .map(|r| match r {
            Row::Header { .. } => 1,
            Row::Session(_) => session_height,
        })
        .collect();

    // Find the row index that corresponds to App.selected (counting sessions only).
    let selected_row_idx = {
        let visible_sessions = app.visible_session_indices();
        let target = visible_sessions.get(app.selected).copied();
        let mut chosen = 0usize;
        for (i, r) in rows.iter().enumerate() {
            if let Row::Session(idx) = r {
                if Some(*idx) == target {
                    chosen = i;
                    break;
                }
            }
        }
        chosen
    };

    // Pick start_row so that [start_row..] cumulatively fits and includes selected_row_idx.
    let mut start_row = 0usize;
    loop {
        let mut used = 0usize;
        let mut last_visible = start_row;
        for (i, h) in row_heights.iter().enumerate().skip(start_row) {
            used += h;
            if used > avail {
                break;
            }
            last_visible = i;
        }
        if selected_row_idx <= last_visible || start_row >= rows.len() - 1 {
            break;
        }
        start_row += 1;
    }

    let mut y = inner.y;
    let max_y = inner.y + inner.height;
    let visible_sessions = app.visible_session_indices();
    let selected_session_real = visible_sessions.get(app.selected).copied();

    for (_ri, row) in rows.iter().enumerate().skip(start_row) {
        let h = match row {
            Row::Header { .. } => 1,
            Row::Session(_) => session_height,
        } as u16;
        if y + h > max_y {
            break;
        }

        match row {
            Row::Header { cwd, total, alive, collapsed } => {
                app.list_hits
                    .borrow_mut()
                    .push((y, 1, RowHit::Header(cwd.clone())));
                draw_group_header(
                    f,
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: 1,
                    },
                    cwd,
                    *total,
                    *alive,
                    *collapsed,
                );
            }
            Row::Session(real_idx) => {
                app.list_hits
                    .borrow_mut()
                    .push((y, h, RowHit::Session(*real_idx)));
                let session = &app.sessions[*real_idx];
                let selected = selected_session_real == Some(*real_idx);
                let tmux_backed = app.tmux_backed.contains(&session.id);
                draw_session_row(
                    f,
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: h,
                    },
                    session,
                    selected,
                    tmux_backed,
                    tier,
                );
            }
        }
        y += h;
    }
}

fn draw_group_header(f: &mut Frame, area: Rect, cwd: &str, total: usize, alive: usize, collapsed: bool) {
    let chevron = if collapsed { "▸" } else { "▾" };
    let name = short_path(cwd);
    let mut spans: Vec<Span> = vec![
        Span::styled(format!(" {} ", chevron), Style::default().fg(ACCENT_DIM)),
        Span::styled(
            truncate(&name, (area.width as usize).saturating_sub(18)),
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];
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
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_session_row(
    f: &mut Frame,
    area: Rect,
    session: &SessionInfo,
    selected: bool,
    tmux_backed: bool,
    tier: Layoutness,
) {
    let bullet = if tmux_backed {
        "▶"
    } else if session.is_alive {
        "●"
    } else {
        "○"
    };
    let bullet_color = if tmux_backed {
        // Distinct teal-ish color so backgrounded tmux sessions pop visually.
        Color::Rgb(0x6F, 0xD9, 0xCB)
    } else if session.is_alive {
        match session.status.as_str() {
            "busy" => WARN,
            "thinking" => Color::Rgb(0xB8, 0xA0, 0xFF),
            _ => LIVE,
        }
    } else if session.is_recently_active() {
        ACCENT
    } else {
        MUTED
    };

    let name_style = if selected {
        Style::default()
            .fg(SEL_FG)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else if session.is_recently_active() {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(MUTED)
    };
    let cost_style = if selected {
        Style::default().fg(SEL_FG).bg(ACCENT)
    } else {
        Style::default().fg(ACCENT_DIM)
    };
    let model_style = if selected {
        Style::default().fg(SEL_FG).bg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };

    let model_label = model_short(session.model.as_deref());
    let cost_str = format!(" ${:>6.2} ", session.cost);
    let model_str = format!(" {} ", model_label);

    // Indent under group header for visual hierarchy.
    let indent = if tier == Layoutness::Narrow { "  " } else { "   " };
    let name_width = (area.width as i32)
        - indent.len() as i32
        - 4
        - cost_str.len() as i32
        - model_str.len() as i32;
    let name = truncate(&session.name, name_width.max(4) as usize);

    let row1 = Line::from(vec![
        Span::raw(indent),
        Span::styled(format!("{} ", bullet), Style::default().fg(bullet_color)),
        Span::styled(name, name_style),
        Span::raw(" "),
        Span::styled(model_str, model_style),
        Span::styled(cost_str, cost_style),
    ]);
    f.render_widget(
        Paragraph::new(row1),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        },
    );

    // Second row: time-ago + status (skipped in narrow tier).
    if matches!(tier, Layoutness::Narrow) || area.height < 2 {
        return;
    }
    let ago = ago_string(session.last_activity_at.as_ref());
    let status_text = if tmux_backed && !session.is_alive {
        "▶ tmux idle".to_string()
    } else if tmux_backed {
        match session.status.as_str() {
            "busy" => "▶ tmux busy".to_string(),
            "thinking" => "▶ tmux thinking".to_string(),
            _ => "▶ tmux idle".to_string(),
        }
    } else if session.is_alive {
        match session.status.as_str() {
            "busy" => "● busy",
            "thinking" => "● thinking",
            "idle" => "● idle",
            other => other,
        }
        .to_string()
    } else {
        String::new()
    };
    let pad = (area.width as usize)
        .saturating_sub(indent.len() + 2 + status_text.chars().count() + ago.chars().count() + 2);
    let row2 = Line::from(vec![
        Span::raw(indent),
        Span::raw("  "),
        Span::styled(status_text, Style::default().fg(bullet_color)),
        Span::raw(" ".repeat(pad)),
        Span::styled(ago, Style::default().fg(MUTED)),
    ]);
    f.render_widget(
        Paragraph::new(row2),
        Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 1,
        },
    );
}

fn draw_detail(f: &mut Frame, area: Rect, app: &App, tier: Layoutness) {
    let block = panel_block("Detail", false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let session = match app.selected_session() {
        Some(s) => s,
        None => {
            let p = Paragraph::new(Span::styled(
                "select a session on the left",
                Style::default().fg(MUTED),
            ))
            .alignment(Alignment::Center);
            f.render_widget(p, inner);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();
    let title_color = if session.is_alive { LIVE } else { ACCENT };
    lines.push(Line::from(vec![
        Span::styled(
            session.name.clone(),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![Span::styled(
        short_path(&session.cwd),
        Style::default().fg(MUTED),
    )]));
    lines.push(Line::raw(""));

    let compact = matches!(tier, Layoutness::Stacked);
    lines.push(meta_row("session", short_id(&session.id)));
    lines.push(meta_row(
        "model",
        session.model.clone().unwrap_or_else(|| "—".into()),
    ));
    if session.is_alive {
        lines.push(meta_row("status", format!("● live (pid {})", session.pid)));
    } else {
        lines.push(meta_row("status", session.status.clone()));
    }
    if !compact {
        if let Some(t) = session.started_at {
            lines.push(meta_row("started", t.format("%Y-%m-%d %H:%M").to_string()));
        }
    }
    if let Some(t) = session.last_activity_at {
        lines.push(meta_row(
            "last activity",
            if compact {
                ago_string(Some(&t))
            } else {
                format!("{}  ({})", t.format("%H:%M:%S"), ago_string(Some(&t)))
            },
        ));
    }
    if !compact && !session.version.is_empty() {
        lines.push(meta_row("claude", session.version.clone()));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "── tokens ──",
        Style::default().fg(ACCENT_DIM),
    )));
    lines.push(token_row("input", session.usage.total_input));
    lines.push(token_row("cache read", session.usage.cache_read));
    lines.push(token_row("cache write", session.usage.cache_creation));
    lines.push(token_row("output", session.usage.total_output));
    lines.push(meta_row(
        "messages",
        session.usage.message_count.to_string(),
    ));
    lines.push(meta_row(
        "cache hit",
        format!("{:.1}%", session.usage.cache_hit_rate() * 100.0),
    ));

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("cost  ", Style::default().fg(MUTED)),
        Span::styled(
            format!("${:.4}", session.cost),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Saved-by-cache delta: estimate based on input vs cache_read at full price.
    let saved = saved_by_cache(session);
    if saved > 0.0001 {
        lines.push(Line::from(vec![
            Span::styled("saved by cache  ", Style::default().fg(MUTED)),
            Span::styled(
                format!("${:.4}", saved),
                Style::default().fg(LIVE).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(p, inner);

    // Token mix gauge under detail content if there's room.
    let gauge_height = 3;
    if inner.height >= 16 + gauge_height {
        let mix_area = Rect {
            x: inner.x,
            y: inner.y + inner.height - gauge_height,
            width: inner.width,
            height: gauge_height,
        };
        draw_token_mix(f, mix_area, session);
    }
}

fn draw_token_mix(f: &mut Frame, area: Rect, s: &SessionInfo) {
    let u = &s.usage;
    let total = (u.total_input + u.cache_read + u.cache_creation + u.total_output) as f64;
    if total < 1.0 {
        return;
    }
    let rd = (u.cache_read as f64 / total * 100.0) as u16;
    let input_pct = (u.total_input as f64 / total * 100.0) as u16;
    let out_pct = (u.total_output as f64 / total * 100.0) as u16;

    let label = format!(
        "cache {}%  ·  input {}%  ·  output {}%",
        rd, input_pct, out_pct
    );
    let g = Gauge::default()
        .block(Block::default().borders(Borders::NONE))
        .gauge_style(Style::default().fg(ACCENT).bg(Color::Rgb(0x22, 0x1E, 0x18)))
        .ratio((rd as f64 / 100.0).clamp(0.0, 1.0))
        .label(Span::styled(label, Style::default().fg(TEXT)));
    f.render_widget(g, area);
}

fn meta_row(key: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<14}", key), Style::default().fg(MUTED)),
        Span::styled(value.into(), Style::default().fg(TEXT)),
    ])
}

fn token_row(label: &str, n: u64) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<14}", label), Style::default().fg(MUTED)),
        Span::styled(
            format_num(n),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn format_num(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect::<String>() + "…"
}

fn saved_by_cache(s: &SessionInfo) -> f64 {
    let (pi, _po, pcr, _pcw) = crate::models::pricing_for(s.model.as_deref());
    let full_price = s.usage.cache_read as f64 / 1_000_000.0 * pi;
    let actual = s.usage.cache_read as f64 / 1_000_000.0 * pcr;
    (full_price - actual).max(0.0)
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn ago_string(t: Option<&chrono::DateTime<chrono::Local>>) -> String {
    let t = match t {
        Some(t) => *t,
        None => return "—".into(),
    };
    let secs = (Local::now() - t).num_seconds();
    if secs < 0 {
        return "now".into();
    }
    if secs < 60 {
        return format!("{}s ago", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m ago", mins);
    }
    let hrs = mins / 60;
    if hrs < 24 {
        return format!("{}h ago", hrs);
    }
    let days = hrs / 24;
    if days < 30 {
        return format!("{}d ago", days);
    }
    t.format("%Y-%m-%d").to_string()
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
        Span::styled(prefix, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(" focus list", Style::default().fg(MUTED)),
        sep(ACCENT_DIM),
        Span::styled("keys", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
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

fn draw_footer(f: &mut Frame, area: Rect, app: &App, tier: Layoutness) {
    // Terminal pane focused: a dedicated footer shows the configured prefix.
    if matches!(app.mode, Mode::Terminal) {
        draw_terminal_footer(f, area, app);
        return;
    }
    let narrow = matches!(tier, Layoutness::Narrow);
    let mut hints: Vec<(&str, &str)> = match app.mode {
        Mode::Browse if narrow => vec![
            ("⏎", "resume"),
            ("n", "claude"),
            ("/", "filter"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Mode::Browse => vec![
            ("↑↓", "nav"),
            ("⏎", "resume"),
            ("n", "new claude"),
            ("s", "new shell"),
            ("/", "filter"),
            ("r", "rename"),
            ("R", "refresh"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Mode::Filter => vec![("⏎", "apply"), ("\\", "AI search"), ("esc", "cancel")],
        Mode::Rename => vec![("⏎", "save"), ("esc", "cancel")],
        Mode::Help | Mode::Confirm(_) => vec![("esc", "close")],
        Mode::Launch(_) => vec![("⏎", "launch"), ("space", "toggle"), ("esc", "cancel")],
        Mode::Settings => vec![("⏎", "save"), ("esc", "cancel")],
        Mode::CostSummary => vec![("esc", "close")],
        // Terminal footer is drawn separately (shows the configured prefix).
        Mode::Terminal => vec![],
    };

    // When a terminal is open but the sidebar is focused, advertise how to jump in.
    if matches!(app.mode, Mode::Browse) && app.has_terminal() {
        hints.insert(0, ("i", "terminal"));
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
        if let Some(s) = app.selected_session() {
            let summary = Line::from(vec![
                Span::styled(
                    model_short(s.model.as_deref()).to_string(),
                    Style::default().fg(ACCENT).bold(),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("${:.4}", s.cost),
                    Style::default().fg(TEXT),
                ),
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

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(4));
    let h = height.min(area.height.saturating_sub(4));
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn draw_filter_overlay(f: &mut Frame, area: Rect, app: &App) {
    // Inline at top of list — small floating bar.
    let bar = Rect {
        x: area.x + 2,
        y: area.y + 4,
        width: area.width.saturating_sub(4).min(60),
        height: 3,
    };
    f.render_widget(Clear, bar);
    let block = panel_block("Filter", true);
    let inner = block.inner(bar);
    f.render_widget(block, bar);
    let line = Line::from(vec![
        Span::styled("› ", Style::default().fg(ACCENT)),
        Span::styled(app.filter.clone(), Style::default().fg(TEXT)),
        Span::styled("▏", Style::default().fg(ACCENT).slow_blink()),
    ]);
    f.render_widget(Paragraph::new(line), inner);
}

fn draw_rename_overlay(f: &mut Frame, area: Rect, app: &App) {
    let bar = centered(area, 60, 5);
    f.render_widget(Clear, bar);
    let block = panel_block("Rename session", true);
    let inner = block.inner(bar);
    f.render_widget(block, bar);
    let line = Line::from(vec![
        Span::styled("name › ", Style::default().fg(ACCENT)),
        Span::styled(app.rename_buf.clone(), Style::default().fg(TEXT)),
        Span::styled("▏", Style::default().fg(ACCENT).slow_blink()),
    ]);
    f.render_widget(Paragraph::new(line), inner);
}

fn draw_cost_summary_overlay(f: &mut Frame, area: Rect, app: &App) {
    use std::collections::HashMap;

    let modal = centered(area, 74, 34);
    f.render_widget(Clear, modal);
    let block = panel_block("Cost summary", true);
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    // Aggregate over all sessions.
    let total: f64 = app.sessions.iter().map(|s| s.cost).sum();
    let today = app.today_cost();

    let mut by_dir: HashMap<String, f64> = HashMap::new();
    let mut by_model: HashMap<&str, f64> = HashMap::new();
    let mut by_day: HashMap<String, f64> = HashMap::new();
    for s in &app.sessions {
        *by_dir.entry(short_path(&s.cwd)).or_insert(0.0) += s.cost;
        *by_model.entry(model_short(s.model.as_deref())).or_insert(0.0) += s.cost;
        for (day, c) in &s.cost_by_day {
            *by_day.entry(day.clone()).or_insert(0.0) += *c;
        }
    }

    // Sort helper: by value desc.
    fn top(map: impl IntoIterator<Item = (String, f64)>, n: usize) -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = map.into_iter().filter(|(_, c)| *c > 0.0).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }
    fn bar(val: f64, max: f64, width: usize) -> String {
        if max <= 0.0 {
            return String::new();
        }
        let filled = ((val / max) * width as f64).round() as usize;
        "█".repeat(filled.min(width))
    }

    let dirs = top(by_dir, 8);
    let models = top(by_model.into_iter().map(|(k, v)| (k.to_string(), v)), 4);
    let mut days: Vec<(String, f64)> =
        by_day.into_iter().filter(|(_, c)| *c > 0.0).collect();
    days.sort_by(|a, b| b.0.cmp(&a.0)); // most recent day first
    days.truncate(10);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("total  ", Style::default().fg(MUTED)),
        Span::styled(
            format!("${:.2}", total),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("    today  ", Style::default().fg(MUTED)),
        Span::styled(format!("${:.2}", today), Style::default().fg(LIVE)),
    ]));

    let section = |lines: &mut Vec<Line>, title: &str, rows: &[(String, f64)]| {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("── {} ──", title),
            Style::default().fg(ACCENT_DIM),
        )));
        let max = rows.iter().map(|(_, c)| *c).fold(0.0_f64, f64::max);
        for (label, cost) in rows {
            let name = truncate(label, 28);
            lines.push(Line::from(vec![
                Span::styled(format!("{:<29}", name), Style::default().fg(TEXT)),
                Span::styled(format!("${:>8.2} ", cost), Style::default().fg(ACCENT)),
                Span::styled(bar(*cost, max, 22), Style::default().fg(ACCENT_DIM)),
            ]));
        }
        if rows.is_empty() {
            lines.push(Line::from(Span::styled("  (none)", Style::default().fg(MUTED))));
        }
    };

    section(&mut lines, "by directory", &dirs);
    section(&mut lines, "by model", &models);
    section(&mut lines, "by day (recent)", &days);

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_settings_overlay(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(area, 64, 14);
    f.render_widget(Clear, modal);
    let block = panel_block("Settings", true);
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let mark = |i: usize| -> Span<'static> {
        if app.settings_field == i {
            Span::styled("▸ ", Style::default().fg(ACCENT))
        } else {
            Span::raw("  ")
        }
    };
    let cursor = |i: usize| -> Span<'static> {
        if app.settings_field == i {
            Span::styled("▏", Style::default().fg(ACCENT).slow_blink())
        } else {
            Span::raw("")
        }
    };
    let budget_shown = if app.settings_budget_input.is_empty() {
        "off".to_string()
    } else {
        app.settings_budget_input.clone()
    };

    let lines = vec![
        Line::from(Span::styled(
            "terminal escape prefix",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            mark(0),
            Span::styled("key  ", Style::default().fg(MUTED)),
            Span::styled(app.settings_input.clone(), Style::default().fg(TEXT)),
            cursor(0),
        ]),
        Line::from(Span::styled(
            "    e.g. ctrl-a  ctrl-b  f12  ctrl-space",
            Style::default().fg(MUTED),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "daily budget (USD)",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            mark(1),
            Span::styled("$    ", Style::default().fg(MUTED)),
            Span::styled(budget_shown, Style::default().fg(TEXT)),
            cursor(1),
        ]),
        Line::from(Span::styled(
            "    alert when today's spend crosses it; blank = off",
            Style::default().fg(MUTED),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("⏎", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(" save   ", Style::default().fg(MUTED)),
            Span::styled("↑↓/tab", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(" field   ", Style::default().fg(MUTED)),
            Span::styled("esc", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel", Style::default().fg(MUTED)),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_help_overlay(f: &mut Frame, area: Rect) {
    let modal = centered(area, 64, 36);
    f.render_widget(Clear, modal);
    let block = panel_block("Help", true);
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let lines = vec![
        Line::from(Span::styled(
            "navigation",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        help_row("↑ ↓ / j k", "move selection"),
        help_row("g / G", "first / last"),
        help_row("space / tab", "collapse/expand group"),
        help_row("o / O", "collapse inactive / expand all"),
        help_row("T", "toggle grouping by directory"),
        Line::raw(""),
        Line::from(Span::styled(
            "session actions",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        help_row("⏎", "resume selected session"),
        help_row("n", "new claude (defaults)"),
        help_row("N", "new claude (with options)"),
        help_row("s", "new shell in cwd"),
        help_row("r", "rename"),
        Line::raw(""),
        Line::from(Span::styled(
            "tmux multi-session",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        help_row("Ctrl-b d", "detach (inside a tmux session)"),
        help_row("⏎", "re-attach an existing ▶ background session"),
        help_row("K", "kill the background tmux session"),
        Line::raw(""),
        Line::from(Span::styled(
            "search & AI",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        help_row("/", "literal filter"),
        help_row("\\", "AI search (Haiku)"),
        help_row("A", "auto-name unnamed sessions"),
        Line::raw(""),
        Line::from(Span::styled(
            "maintenance",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        help_row("D", "delete junk sessions"),
        help_row("E", "delete empty sessions"),
        help_row("M", "toggle desktop notifications"),
        help_row("R", "refresh now"),
        help_row(":", "settings (terminal prefix)"),
        help_row("c", "cost summary"),
        help_row("i / l", "focus embedded terminal"),
        help_row("?", "this help"),
        help_row("q / ctrl-c", "quit"),
    ];
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(p, inner);
}

fn help_row(keys: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<14}", keys),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc.to_string(), Style::default().fg(TEXT)),
    ])
}

fn draw_confirm_overlay(f: &mut Frame, area: Rect, app: &App) {
    let prompt = match &app.mode {
        Mode::Confirm(a) => a.prompt(),
        _ => "Confirm?",
    };
    let modal = centered(area, 60, 7);
    f.render_widget(Clear, modal);
    let block = panel_block("Confirm", true);
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    let lines = vec![
        Line::raw(""),
        Line::from(Span::styled(prompt, Style::default().fg(TEXT))),
        Line::raw(""),
        Line::from(vec![
            Span::styled("y", Style::default().fg(ACCENT).bold()),
            Span::styled(" yes   ", Style::default().fg(MUTED)),
            Span::styled("n", Style::default().fg(ACCENT).bold()),
            Span::styled(" no", Style::default().fg(MUTED)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        inner,
    );
}

fn draw_launch_overlay(f: &mut Frame, area: Rect, form: &LaunchForm) {
    let modal = centered(area, 64, 16);
    f.render_widget(Clear, modal);
    let block = panel_block("Launch new claude", true);
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("cwd  ", Style::default().fg(MUTED)),
        Span::styled(short_path(&form.cwd), Style::default().fg(TEXT)),
    ]));
    lines.push(Line::raw(""));

    for i in 0..LaunchForm::FIELD_COUNT {
        let focused = i == form.field;
        let label_style = if focused {
            Style::default().fg(SEL_FG).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let value_style = if focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT)
        };
        let marker = if focused { "▸ " } else { "  " };
        let label = form.field_label(i);
        let value = form.field_value(i);
        lines.push(Line::from(vec![
            Span::raw(marker),
            Span::styled(format!(" {:<32}", label), label_style),
            Span::raw("  "),
            Span::styled(value, value_style),
            if (i == 0 || i == 5) && focused {
                Span::styled("▏", Style::default().fg(ACCENT).slow_blink())
            } else {
                Span::raw("")
            },
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("⏎", Style::default().fg(ACCENT).bold()),
        Span::styled(" launch  ", Style::default().fg(MUTED)),
        Span::styled("space/←→", Style::default().fg(ACCENT).bold()),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT).bold()),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_toast(f: &mut Frame, area: Rect, msg: &str) {
    let w = (msg.chars().count() as u16 + 6).min(area.width.saturating_sub(4));
    let toast = Rect {
        x: area.x + area.width.saturating_sub(w + 2),
        y: area.y + area.height.saturating_sub(4),
        width: w,
        height: 3,
    };
    f.render_widget(Clear, toast);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(toast);
    f.render_widget(block, toast);
    f.render_widget(
        Paragraph::new(Span::styled(msg, Style::default().fg(TEXT)))
            .alignment(Alignment::Center),
        inner,
    );
}
