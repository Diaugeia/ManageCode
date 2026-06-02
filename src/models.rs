use std::collections::HashMap;

use chrono::{DateTime, Local};

#[derive(Debug, Default, Clone, Copy)]
pub struct TokenUsage {
    pub total_input: u64,
    pub total_output: u64,
    pub cache_read: u64,
    /// Cache-write (cache-creation) tokens valid for 5 minutes.
    pub cache_creation_5m: u64,
    /// Cache-write tokens valid for 1 hour (billed at a higher rate than 5m).
    pub cache_creation_1h: u64,
    pub message_count: u64,
}

impl TokenUsage {
    /// Total cache-write tokens across both the 5-minute and 1-hour tiers.
    pub fn cache_creation(&self) -> u64 {
        self.cache_creation_5m + self.cache_creation_1h
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_read + self.cache_creation() + self.total_input;
        if total == 0 {
            0.0
        } else {
            self.cache_read as f64 / total as f64
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub pid: i32,
    pub name: String,
    pub cwd: String,
    pub status: String,
    pub started_at: Option<DateTime<Local>>,
    pub last_activity_at: Option<DateTime<Local>>,
    pub version: String,
    pub model: Option<String>,
    pub usage: TokenUsage,
    pub cost: f64,
    /// Cost accrued per local calendar day ("YYYY-MM-DD" -> USD).
    pub cost_by_day: HashMap<String, f64>,
    pub is_alive: bool,
}

impl SessionInfo {
    pub fn is_recently_active(&self) -> bool {
        if self.is_alive {
            return true;
        }
        match self.last_activity_at {
            Some(t) => (Local::now() - t).num_seconds() < 3600,
            None => false,
        }
    }

    /// Cost this session accrued today (local calendar day).
    pub fn cost_today(&self) -> f64 {
        let today = Local::now().format("%Y-%m-%d").to_string();
        self.cost_by_day.get(&today).copied().unwrap_or(0.0)
    }
}

/// Local calendar-day key, e.g. "2026-05-31".
pub fn day_key(t: &DateTime<Local>) -> String {
    t.format("%Y-%m-%d").to_string()
}

/// Public per-million-token pricing for a model id.
/// Returns `(input, output, cache_read, cache_write_5m, cache_write_1h)`.
///
/// Anthropic models carry two cache-write tiers (5-minute and 1-hour); OpenAI /
/// Codex models have no cache-write charge, so both write tiers are 0 and the
/// `cache_read` figure is the cached-input discount rate.
pub fn pricing_for(model: Option<&str>) -> (f64, f64, f64, f64, f64) {
    let m = model.map(|s| s.to_ascii_lowercase()).unwrap_or_default();

    // OpenAI / Codex models: (input, output, cached_input, 0, 0).
    if m.contains("gpt")
        || m.contains("codex")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
    {
        if m.contains("5.5") {
            return (5.0, 30.0, 0.5, 0.0, 0.0);
        } else if m.contains("5.4") {
            return (2.5, 15.0, 0.25, 0.0, 0.0);
        } else if m.contains("4o") {
            return (2.5, 10.0, 1.25, 0.0, 0.0);
        }
        // Default unknown gpt-5.x ids to the current Codex model (gpt-5.5).
        return (5.0, 30.0, 0.5, 0.0, 0.0);
    }

    // Anthropic models. Cache-write 5m ≈ 1.25× input, 1h ≈ 2× input.
    if m.contains("opus") {
        (5.0, 25.0, 0.5, 6.25, 10.0) // Opus 4.5+
    } else if m.contains("haiku") {
        (1.0, 5.0, 0.1, 1.25, 2.0) // Haiku 4.5
    } else {
        (3.0, 15.0, 0.3, 3.75, 6.0) // Sonnet 4.x (also the fallback default)
    }
}

pub fn cost_for(u: &TokenUsage, model: Option<&str>) -> f64 {
    let (pi, po, pcr, pcw5, pcw1) = pricing_for(model);
    (u.total_input as f64) / 1_000_000.0 * pi
        + (u.total_output as f64) / 1_000_000.0 * po
        + (u.cache_read as f64) / 1_000_000.0 * pcr
        + (u.cache_creation_5m as f64) / 1_000_000.0 * pcw5
        + (u.cache_creation_1h as f64) / 1_000_000.0 * pcw1
}

pub fn short_path(p: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let h = home.to_string_lossy().to_string();
        if let Some(rest) = p.strip_prefix(&h) {
            return format!("~{}", rest);
        }
    }
    p.to_string()
}

pub fn model_short(m: Option<&str>) -> &'static str {
    let m = m.map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if m.contains("opus") {
        "opus"
    } else if m.contains("sonnet") {
        "sonnet"
    } else if m.contains("haiku") {
        "haiku"
    } else if m.contains("gpt") || m.contains("codex") {
        "gpt"
    } else {
        "?"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64, cr: u64, cw5: u64, cw1: u64) -> TokenUsage {
        TokenUsage {
            total_input: input,
            total_output: output,
            cache_read: cr,
            cache_creation_5m: cw5,
            cache_creation_1h: cw1,
            message_count: 1,
        }
    }

    #[test]
    fn sonnet_input_output_cost() {
        // 1M input @ $3 + 1M output @ $15 = $18.
        let c = cost_for(&usage(1_000_000, 1_000_000, 0, 0, 0), Some("claude-sonnet-4-6"));
        assert!((c - 18.0).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn cache_write_tiers_differ() {
        // Sonnet 5m write $3.75/M vs 1h write $6/M — must not be conflated.
        assert!((cost_for(&usage(0, 0, 0, 1_000_000, 0), Some("sonnet")) - 3.75).abs() < 1e-9);
        assert!((cost_for(&usage(0, 0, 0, 0, 1_000_000), Some("sonnet")) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn haiku_uses_4_5_not_3_5() {
        // Haiku 4.5 input is $1/M, not the retired 3.5 rate of $0.80/M.
        assert!((cost_for(&usage(1_000_000, 0, 0, 0, 0), Some("claude-haiku-4-5")) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn opus_uses_current_not_4_1() {
        // Current Opus input is $5/M, not the deprecated 4.1 rate of $15/M.
        assert!((cost_for(&usage(1_000_000, 0, 0, 0, 0), Some("claude-opus-4-8")) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn cache_read_is_discounted() {
        assert!((cost_for(&usage(0, 0, 1_000_000, 0, 0), Some("sonnet")) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn codex_gpt55_priced_with_openai_rates() {
        // gpt-5.5: $5 input + $30 output + $0.50 cached read, no cache-write charge.
        let c = cost_for(&usage(1_000_000, 1_000_000, 1_000_000, 0, 0), Some("gpt-5.5"));
        assert!((c - 35.5).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn unknown_model_falls_back_to_sonnet() {
        assert!((cost_for(&usage(1_000_000, 0, 0, 0, 0), None) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn cache_creation_sums_both_tiers() {
        let u = usage(0, 0, 0, 100, 200);
        assert_eq!(u.cache_creation(), 300);
    }
}
