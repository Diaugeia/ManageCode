//! Top header bar: title + live stats line. Split out of the parent ui module
//! to keep it scannable; everything here is internal to `ui`.

use super::*;

pub(super) fn draw_header(f: &mut Frame, area: Rect, app: &App, tier: Layoutness) {
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
            spans.push(Span::styled(busy_label.clone(), Style::default().fg(WARN)));
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
                    (format!("today ${:.2}/{:.2}", today, limit), c)
                }
                _ => (format!("today ${:.2}", today), LIVE),
            };
            spans.push(Span::styled(
                txt,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
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
