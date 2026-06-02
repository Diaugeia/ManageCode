//! Session list pane: the scrollable sidebar of group headers, tree nodes, and
//! session rows. Split out of the parent ui module to keep it scannable;
//! everything here is internal to `ui`.

use super::detail::{ago_string, truncate};
use super::*;

pub(super) fn draw_session_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    tier: Layoutness,
    rows: &[Row],
) {
    let block = panel_block("Sessions", matches!(app.mode, Mode::Browse | Mode::Filter));
    let inner = block.inner(area);
    f.render_widget(block, area);

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
    let session_height: usize = if matches!(tier, Layoutness::Narrow) {
        1
    } else {
        2
    };

    // Walk rows once to compute heights, then pick a viewport that keeps the
    // currently selected session row in view.
    let row_heights: Vec<usize> = rows
        .iter()
        .map(|r| match r {
            Row::Header { .. } => 1,
            Row::Tree { .. } => 1,
            Row::Session { .. } => session_height,
        })
        .collect();

    // Selection indexes every visible row directly (headers, tree nodes,
    // sessions), so the cursor can land on — and reopen — a collapsed directory.
    let selected_row_idx = app.selected.min(rows.len().saturating_sub(1));

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

    for (ri, row) in rows.iter().enumerate().skip(start_row) {
        let h = match row {
            Row::Header { .. } => 1,
            Row::Tree { .. } => 1,
            Row::Session { .. } => session_height,
        } as u16;
        if y + h > max_y {
            break;
        }
        let selected = ri == selected_row_idx;

        match row {
            Row::Header {
                cwd,
                total,
                alive,
                collapsed,
            } => {
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
                    selected,
                );
            }
            Row::Tree {
                path,
                name,
                depth,
                total,
                alive,
                collapsed,
            } => {
                app.list_hits
                    .borrow_mut()
                    .push((y, 1, RowHit::Header(path.clone())));
                draw_tree_row(
                    f,
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: 1,
                    },
                    name,
                    *depth,
                    *total,
                    *alive,
                    *collapsed,
                    selected,
                );
            }
            Row::Session {
                idx: real_idx,
                depth,
            } => {
                app.list_hits
                    .borrow_mut()
                    .push((y, h, RowHit::Session(*real_idx)));
                let session = &app.sessions[*real_idx];
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
                    *depth,
                    app.config.show_cost,
                );
            }
        }
        y += h;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_group_header(
    f: &mut Frame,
    area: Rect,
    cwd: &str,
    total: usize,
    alive: usize,
    collapsed: bool,
    selected: bool,
) {
    let chevron = if collapsed { "▸" } else { "▾" };
    let name = short_path(cwd);
    let chevron_style = if selected {
        sel_style(false)
    } else {
        Style::default().fg(ACCENT_DIM)
    };
    let name_style = if selected {
        sel_style(true)
    } else {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    };
    let mut spans: Vec<Span> = vec![
        Span::styled(format!(" {} ", chevron), chevron_style),
        Span::styled(
            truncate(&name, (area.width as usize).saturating_sub(18)),
            name_style,
        ),
    ];
    spans.extend(count_spans(alive, total));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tree_row(
    f: &mut Frame,
    area: Rect,
    name: &str,
    depth: usize,
    total: usize,
    alive: usize,
    collapsed: bool,
    selected: bool,
) {
    let chevron = if collapsed { "▸" } else { "▾" };
    let indent = "  ".repeat(depth);
    let avail = (area.width as usize).saturating_sub(depth * 2 + 12);
    let chevron_style = if selected {
        sel_style(false)
    } else {
        Style::default().fg(ACCENT_DIM)
    };
    let name_style = if selected {
        sel_style(true)
    } else {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    };
    let mut spans: Vec<Span> = vec![
        Span::raw(format!(" {}", indent)),
        Span::styled(format!("{} ", chevron), chevron_style),
        Span::styled(truncate(name, avail.max(4)), name_style),
    ];
    spans.extend(count_spans(alive, total));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_session_row(
    f: &mut Frame,
    area: Rect,
    session: &SessionInfo,
    selected: bool,
    tmux_backed: bool,
    tier: Layoutness,
    depth: usize,
    show_cost: bool,
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
        sel_style(true)
    } else if session.is_recently_active() {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(MUTED)
    };
    let cost_style = if selected {
        sel_style(false)
    } else {
        Style::default().fg(ACCENT_DIM)
    };
    let model_style = if selected {
        sel_style(false)
    } else {
        Style::default().fg(MUTED)
    };

    let model_label = model_short(session.model.as_deref());
    let model_str = format!(" {} ", model_label);
    // Cost is opt-in (config.show_cost) and right-aligned to the far edge.
    let cost_str = if show_cost {
        format!(" ${:>6.2} ", session.cost)
    } else {
        String::new()
    };

    // Base indent gives the group/flat hierarchy; `depth` adds tree nesting.
    let base = if tier == Layoutness::Narrow { 2 } else { 3 };
    let indent = " ".repeat(base + depth * 2);
    // bullet span is "{glyph} " = 2 cols.
    let right = model_str.chars().count() + cost_str.chars().count();
    let avail = (area.width as usize).saturating_sub(indent.len() + 2 + right);
    let name = truncate(&session.name, avail.max(4));
    // Pad between name and the right-aligned model/cost block.
    let pad = avail.saturating_sub(name.chars().count());

    let row1 = Line::from(vec![
        Span::raw(indent.clone()),
        Span::styled(format!("{} ", bullet), Style::default().fg(bullet_color)),
        Span::styled(name, name_style),
        // Highlight the gap too when selected, for a continuous selection bar.
        Span::styled(" ".repeat(pad), name_style),
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
