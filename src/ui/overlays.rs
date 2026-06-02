//! Modal/overlay renderers (filter, rename, cost summary, settings, help,
//! confirm, launch, toast) and the `centered` rect helper. Split out of the
//! parent ui module to keep it scannable; everything here is internal to `ui`.

use super::*;

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

pub(super) fn draw_filter_overlay(f: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_rename_overlay(f: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_migrate_overlay(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(area, 70, 7);
    f.render_widget(Clear, modal);
    let block = panel_block("Migrate memory", true);
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    let lines = vec![
        Line::from(vec![
            Span::styled("from  ", Style::default().fg(MUTED)),
            Span::styled(short_path(&app.migrate_src), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("to  › ", Style::default().fg(ACCENT)),
            Span::styled(short_path(&app.migrate_input), Style::default().fg(TEXT)),
            Span::styled("▏", Style::default().fg(ACCENT).slow_blink()),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "copies CLAUDE.md + AGENTS.md  ·  ←→ recent dirs  ·  ⏎ migrate",
            Style::default().fg(MUTED),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw_cost_summary_overlay(f: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_settings_overlay(f: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_help_overlay(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(area, 64, 36);
    f.render_widget(Clear, modal);
    let block = panel_block("Help", true);
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    // Generated from the central keymap so it always matches the real bindings.
    let rows = app.keymap.help_rows();
    let mut lines: Vec<Line> = Vec::new();
    let mut first = true;
    for group in crate::keymap::GROUPS {
        let in_group: Vec<&crate::keymap::HelpRow> =
            rows.iter().filter(|r| r.group == *group).collect();
        if in_group.is_empty() {
            continue;
        }
        if !first {
            lines.push(Line::raw(""));
        }
        first = false;
        lines.push(Line::from(Span::styled(
            *group,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        for r in in_group {
            lines.push(help_row(&r.keys, r.desc));
        }
        // Informational (not a binding): how to detach a backgrounded session.
        if *group == "tmux multi-session" {
            lines.push(help_row("Ctrl-a / Ctrl-b d", "detach (keeps it running)"));
        }
    }
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

pub(super) fn draw_confirm_overlay(f: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn draw_launch_overlay(f: &mut Frame, area: Rect, form: &LaunchForm) {
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

pub(super) fn draw_toast(f: &mut Frame, area: Rect, msg: &str) {
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
