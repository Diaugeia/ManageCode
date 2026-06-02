use chrono::{DateTime, Local};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::models::{cost_for, day_key, short_path, SessionInfo, Source, TokenUsage};

use super::{cache_get, cache_put, codex_dir, datetime_of, is_junk_cwd, JsonlCache, ScanOpts};

/// Extract the session UUID from a Codex rollout filename, which looks like
/// `rollout-2026-06-01T22-42-08-019e8635-bb96-7e23-9590-e551cb9e2806.jsonl`.
/// The UUID is the trailing five dash-separated groups (8-4-4-4-12).
pub fn codex_id_from_filename(fname: &str) -> Option<String> {
    let stem = fname.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 5 {
        return None;
    }
    let uuid = parts[parts.len() - 5..].join("-");
    if uuid.len() == 36 {
        Some(uuid)
    } else {
        None
    }
}

/// Pull `(input_tokens, cached_input_tokens, output_tokens)` out of a Codex
/// token-usage object. `output_tokens` already includes reasoning tokens.
fn codex_triple(v: &Value) -> (u64, u64, u64) {
    let g = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    (
        g("input_tokens"),
        g("cached_input_tokens"),
        g("output_tokens"),
    )
}

/// Map Codex token totals onto our unified TokenUsage. OpenAI bills the
/// non-cached input, the cached input at a discount, and output (incl.
/// reasoning); there is no cache-write charge.
fn codex_usage(input: u64, cached: u64, output: u64, messages: u64) -> TokenUsage {
    TokenUsage {
        total_input: input.saturating_sub(cached),
        total_output: output,
        cache_read: cached,
        cache_creation_5m: 0,
        cache_creation_1h: 0,
        message_count: messages,
    }
}

/// Scan `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` and merge the sessions
/// into `by_id`. Honors the same history horizon and size cap as the Claude
/// history scan.
pub(super) fn scan_codex(
    opts: &ScanOpts,
    names: &HashMap<String, String>,
    by_id: &mut HashMap<String, SessionInfo>,
    seen: &mut HashSet<String>,
) {
    let sessions_dir = codex_dir().join("sessions");
    if !sessions_dir.is_dir() {
        return;
    }
    let horizon = Local::now() - chrono::Duration::days(opts.history_days);
    let max_bytes: u64 = opts.max_jsonl_bytes;

    for entry in walkdir::WalkDir::new(&sessions_dir)
        .max_depth(5)
        .into_iter()
        .flatten()
    {
        let path = entry.path();
        let fname = match path.file_name().and_then(|s| s.to_str()) {
            Some(f) if f.starts_with("rollout-") && f.ends_with(".jsonl") => f,
            _ => continue,
        };
        // Cheap filename checks before any stat: skip files we can't key or have
        // already seen.
        let Some(id) = codex_id_from_filename(fname) else {
            continue;
        };
        if seen.contains(&id) {
            continue;
        }
        let attrs = match fs::metadata(path) {
            Ok(a) => a,
            Err(_) => continue,
        };
        if attrs.len() > max_bytes {
            continue;
        }
        let mtime = match attrs.modified() {
            Ok(m) => datetime_of(m),
            Err(_) => continue,
        };
        if mtime < horizon {
            continue;
        }
        if let Some(s) = parse_codex_rollout(path, &id, names, &attrs, mtime) {
            if is_junk_cwd(&s.cwd) {
                continue;
            }
            seen.insert(id.clone());
            by_id.insert(id, s);
        }
    }
}

/// Parse a single Codex rollout file into a SessionInfo. Returns None if the
/// file can't be read. Caches the parse by (size, mtime) like the Claude path.
fn parse_codex_rollout(
    path: &Path,
    id: &str,
    names: &HashMap<String, String>,
    attrs: &fs::Metadata,
    mtime: DateTime<Local>,
) -> Option<SessionInfo> {
    let fingerprint = format!(
        "codex:{}:{}",
        attrs.len(),
        attrs
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    );

    let build = |usage: TokenUsage,
                 model: Option<String>,
                 name_hint: Option<String>,
                 cwd: Option<String>,
                 cost_by_day: HashMap<String, f64>|
     -> SessionInfo {
        let cwd = cwd.unwrap_or_default();
        let cost = cost_for(&usage, model.as_deref());
        let name = names
            .get(id)
            .cloned()
            .or(name_hint)
            .unwrap_or_else(|| short_path(&cwd));
        SessionInfo {
            source: Source::Codex,
            id: id.to_string(),
            pid: 0,
            name,
            cwd,
            status: "ended".to_string(),
            started_at: Some(mtime),
            last_activity_at: Some(mtime),
            version: String::new(),
            model,
            usage,
            cost,
            cost_by_day,
            is_alive: false,
        }
    };

    if let Some(c) = cache_get(id) {
        if c.fingerprint == fingerprint {
            return Some(build(c.usage, c.model, c.ai_title, c.cwd, c.cost_by_day));
        }
    }

    let content = fs::read_to_string(path).ok()?;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut first_user: Option<String> = None;
    let mut messages: u64 = 0;
    let mut final_total = (0u64, 0u64, 0u64);
    // Per-turn usage tagged with the line's timestamp, for daily bucketing.
    let mut turns: Vec<(String, (u64, u64, u64))> = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let obj: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ts = obj.get("timestamp").and_then(|v| v.as_str());
        let ty = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let payload = obj.get("payload");
        match ty {
            "session_meta" => {
                if let Some(p) = payload {
                    if cwd.is_none() {
                        cwd = p.get("cwd").and_then(|v| v.as_str()).map(String::from);
                    }
                }
            }
            "turn_context" => {
                if let Some(m) = payload
                    .and_then(|p| p.get("model"))
                    .and_then(|v| v.as_str())
                {
                    model = Some(m.to_string());
                }
            }
            "event_msg" => {
                let pt = payload
                    .and_then(|p| p.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match pt {
                    "token_count" => {
                        if let Some(info) = payload.and_then(|p| p.get("info")) {
                            if let Some(tot) = info.get("total_token_usage") {
                                final_total = codex_triple(tot);
                            }
                            if let (Some(last), Some(ts)) = (info.get("last_token_usage"), ts) {
                                turns.push((ts.to_string(), codex_triple(last)));
                            }
                        }
                    }
                    "user_message" => {
                        messages += 1;
                        if first_user.is_none() {
                            first_user = payload
                                .and_then(|p| p.get("message"))
                                .and_then(|v| v.as_str())
                                .map(|s| {
                                    let t = s.trim().replace('\n', " ");
                                    t.chars().take(80).collect::<String>()
                                })
                                .filter(|s| !s.is_empty());
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let (ti, tc, to) = final_total;
    let usage = codex_usage(ti, tc, to, messages);

    // Daily cost buckets from per-turn usage at the session's model price.
    let (pi, po, pcr, _, _) = crate::models::pricing_for(model.as_deref());
    let mut cost_by_day: HashMap<String, f64> = HashMap::new();
    for (ts, (i, c, o)) in &turns {
        if let Some(day) = DateTime::parse_from_rfc3339(ts)
            .ok()
            .map(|d| day_key(&d.with_timezone(&Local)))
        {
            let uncached = i.saturating_sub(*c);
            let turn_cost = (uncached as f64) / 1_000_000.0 * pi
                + (*c as f64) / 1_000_000.0 * pcr
                + (*o as f64) / 1_000_000.0 * po;
            *cost_by_day.entry(day).or_insert(0.0) += turn_cost;
        }
    }

    cache_put(
        id,
        JsonlCache {
            fingerprint,
            usage,
            model: model.clone(),
            ai_title: first_user.clone(),
            cwd: cwd.clone(),
            cost_by_day: cost_by_day.clone(),
        },
    );

    Some(build(usage, model, first_user, cwd, cost_by_day))
}
