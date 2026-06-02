//! Detail pane: full metadata, token breakdown, cost, and the token-mix gauge
//! for the selected session. Split out of the parent ui module to keep it
//! scannable; everything here is internal to `ui`.

use super::*;

pub(super) fn draw_detail(f: &mut Frame, area: Rect, app: &App, tier: Layoutness, rows: &[Row]) {
    let block = panel_block("Detail", false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let session = match session_at(app, rows, app.selected) {
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
    let inner_w = inner.width as usize;
    let title_color = if session.is_alive { LIVE } else { ACCENT };
    lines.push(Line::from(vec![Span::styled(
        truncate(&session.name, inner_w),
        Style::default()
            .fg(title_color)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(
        truncate(&short_path(&session.cwd), inner_w),
        Style::default().fg(MUTED),
    )]));
    lines.push(Line::raw(""));

    let compact = matches!(tier, Layoutness::Stacked);
    // ID block: source tag + full id, then the exact resume command (so it's
    // readable/copyable rather than a truncated "abc1234…").
    lines.push(meta_row("source", session.source.tag().to_string()));
    lines.push(meta_row(
        "id",
        truncate(&session.id, inner_w.saturating_sub(15)),
    ));
    lines.push(meta_row(
        "resume",
        truncate(&resume_hint(session), inner_w.saturating_sub(15)),
    ));
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
    lines.push(token_row("cache write", session.usage.cache_creation()));
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
            fmt_usd(session.cost),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Saved-by-cache delta: estimate based on input vs cache_read at full price.
    let saved = saved_by_cache(session);
    if saved > 0.0001 {
        lines.push(Line::from(vec![
            Span::styled("saved by cache  ", Style::default().fg(MUTED)),
            Span::styled(
                fmt_usd(saved),
                Style::default().fg(LIVE).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // No wrap: each line is already truncated to the panel width, so a long id
    // / path / model clips cleanly at the edge instead of wrapping oddly.
    f.render_widget(Paragraph::new(lines), inner);

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

pub(super) fn draw_token_mix(f: &mut Frame, area: Rect, s: &SessionInfo) {
    let u = &s.usage;
    let total = (u.total_input + u.cache_read + u.cache_creation() + u.total_output) as f64;
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

pub(super) fn meta_row(key: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<14}", key), Style::default().fg(MUTED)),
        Span::styled(value.into(), Style::default().fg(TEXT)),
    ])
}

pub(super) fn token_row(label: &str, n: u64) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<14}", label), Style::default().fg(MUTED)),
        Span::styled(
            format_num(n),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
    ])
}

pub(super) fn format_num(n: u64) -> String {
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

/// The exact CLI command to resume this session, by its owning tool.
pub(super) fn resume_hint(s: &SessionInfo) -> String {
    match s.source {
        crate::models::Source::Claude => format!("claude --resume {}", s.id),
        crate::models::Source::Codex => format!("codex resume {}", s.id),
    }
}

pub(super) fn saved_by_cache(s: &SessionInfo) -> f64 {
    let (pi, _po, pcr, _pcw5, _pcw1) = crate::models::pricing_for(s.model.as_deref());
    let full_price = s.usage.cache_read as f64 / 1_000_000.0 * pi;
    let actual = s.usage.cache_read as f64 / 1_000_000.0 * pcr;
    (full_price - actual).max(0.0)
}

pub(super) fn truncate(s: &str, max: usize) -> String {
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

pub(super) fn ago_string(t: Option<&chrono::DateTime<chrono::Local>>) -> String {
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
