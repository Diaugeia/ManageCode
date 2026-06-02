use chrono::{DateTime, Local};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::models::TokenUsage;

use super::{cache_get, cache_put, datetime_of, JsonlCache};

/// Parsed usage + metadata for one Claude session JSONL.
#[derive(Default)]
pub(super) struct ParsedSession {
    pub(super) usage: TokenUsage,
    pub(super) model: Option<String>,
    pub(super) ai_title: Option<String>,
    pub(super) last_modified: Option<DateTime<Local>>,
    pub(super) cwd: Option<String>,
    pub(super) cost_by_day: HashMap<String, f64>,
}

pub(super) fn parse_usage_with_meta(jsonl_path: &Path) -> ParsedSession {
    let session_id = jsonl_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let attrs = match fs::metadata(jsonl_path) {
        Ok(a) => a,
        Err(_) => return ParsedSession::default(),
    };
    let size = attrs.len();
    let mtime = attrs.modified().ok();
    let mtime_dt = mtime.map(datetime_of);
    let fingerprint = match mtime {
        Some(t) => format!(
            "{}:{}",
            size,
            t.duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        ),
        None => format!("{}:0", size),
    };

    if let Some(cached) = cache_get(&session_id) {
        if cached.fingerprint == fingerprint {
            return ParsedSession {
                usage: cached.usage,
                model: cached.model,
                ai_title: cached.ai_title,
                last_modified: mtime_dt,
                cwd: cached.cwd,
                cost_by_day: cached.cost_by_day,
            };
        }
    }

    let content = match fs::read_to_string(jsonl_path) {
        Ok(c) => c,
        Err(_) => {
            return ParsedSession {
                last_modified: mtime_dt,
                ..ParsedSession::default()
            }
        }
    };

    let mut usage = TokenUsage::default();
    let mut model: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut cost_by_day: HashMap<String, f64> = HashMap::new();

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let obj: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if cwd.is_none() {
            if let Some(c) = obj.get("cwd").and_then(|v| v.as_str()) {
                cwd = Some(c.to_string());
            }
        }
        if obj.get("type").and_then(|v| v.as_str()) == Some("ai-title") {
            if let Some(t) = obj.get("aiTitle").and_then(|v| v.as_str()) {
                ai_title = Some(t.to_string());
            }
        }
        if obj.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let msg = match obj.get("message").and_then(|v| v.as_object()) {
            Some(m) => m,
            None => continue,
        };
        let u = match msg.get("usage").and_then(|v| v.as_object()) {
            Some(u) => u,
            None => continue,
        };
        let in_t = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let out_t = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cr_t = u
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        // Cache-write tokens split into 5-minute and 1-hour tiers (the 1h tier
        // bills higher). Newer Claude usage records the split under
        // `cache_creation`; older records only carry the aggregate, which we
        // treat as the 5-minute tier.
        let cc_total = u
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let (cc_5m, cc_1h) = match u.get("cache_creation").and_then(|v| v.as_object()) {
            Some(cc) => {
                let cc5 = cc
                    .get("ephemeral_5m_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let cc1 = cc
                    .get("ephemeral_1h_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                // An empty or partial `cache_creation` object would otherwise
                // drop the write tokens entirely — fall back to the aggregate.
                if cc5 == 0 && cc1 == 0 {
                    (cc_total, 0)
                } else {
                    (cc5, cc1)
                }
            }
            None => (cc_total, 0),
        };
        usage.total_input += in_t;
        usage.total_output += out_t;
        usage.cache_read += cr_t;
        usage.cache_creation_5m += cc_5m;
        usage.cache_creation_1h += cc_1h;
        if obj.get("isSidechain").and_then(|v| v.as_bool()) != Some(true) {
            usage.message_count += 1;
        }
        let msg_model = msg.get("model").and_then(|v| v.as_str()).map(String::from);
        if let Some(m) = &msg_model {
            model = Some(m.clone());
        }

        // Bucket this message's cost by the day it was sent (local time).
        let (pi, po, pcr, pcw5, pcw1) =
            crate::models::pricing_for(msg_model.as_deref().or(model.as_deref()));
        let msg_cost = (in_t as f64) / 1_000_000.0 * pi
            + (out_t as f64) / 1_000_000.0 * po
            + (cr_t as f64) / 1_000_000.0 * pcr
            + (cc_5m as f64) / 1_000_000.0 * pcw5
            + (cc_1h as f64) / 1_000_000.0 * pcw1;
        if let Some(day) = obj
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| crate::models::day_key(&dt.with_timezone(&Local)))
        {
            *cost_by_day.entry(day).or_insert(0.0) += msg_cost;
        }
    }

    cache_put(
        &session_id,
        JsonlCache {
            fingerprint,
            usage,
            model: model.clone(),
            ai_title: ai_title.clone(),
            cwd: cwd.clone(),
            cost_by_day: cost_by_day.clone(),
        },
    );

    ParsedSession {
        usage,
        model,
        ai_title,
        last_modified: mtime_dt,
        cwd,
        cost_by_day,
    }
}
