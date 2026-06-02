use anyhow::Result;
use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{cost_for, day_key, short_path, SessionInfo, Source, TokenUsage};

#[derive(Debug, Clone)]
struct JsonlCache {
    fingerprint: String,
    usage: TokenUsage,
    model: Option<String>,
    ai_title: Option<String>,
    cwd: Option<String>,
    cost_by_day: HashMap<String, f64>,
}

static CACHE: Mutex<Option<HashMap<String, JsonlCache>>> = Mutex::new(None);

fn cache_get(session_id: &str) -> Option<JsonlCache> {
    let mut g = CACHE.lock().ok()?;
    g.get_or_insert_with(HashMap::new).get(session_id).cloned()
}

fn cache_put(session_id: &str, entry: JsonlCache) {
    if let Ok(mut g) = CACHE.lock() {
        g.get_or_insert_with(HashMap::new)
            .insert(session_id.to_string(), entry);
    }
}

pub fn claude_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".claude"))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

pub fn codex_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn names_file() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".managecode").join("session-names.json"))
        .unwrap_or_else(|| PathBuf::from("session-names.json"))
}

pub fn load_custom_names() -> HashMap<String, String> {
    let path = names_file();
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_custom_names(names: &HashMap<String, String>) -> Result<()> {
    let path = names_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(names)?;
    fs::write(&path, data)?;
    Ok(())
}

pub fn is_junk_cwd(cwd: &str) -> bool {
    cwd.is_empty()
        || cwd.starts_with("/private/var/folders/")
        || cwd.starts_with("/var/folders/")
        || cwd.starts_with("/tmp/")
}

#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe { libc_kill(pid, 0) == 0 }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

#[cfg(not(unix))]
fn pid_alive(_pid: i32) -> bool {
    false
}

fn cwd_from_project_name(name: &str) -> String {
    let s = name.trim_start_matches('-');
    let mut out = String::from("/");
    out.push_str(&s.replace('-', "/"));
    out
}

fn datetime_of(t: SystemTime) -> DateTime<Local> {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    Local
        .timestamp_opt(d.as_secs() as i64, d.subsec_nanos())
        .single()
        .unwrap_or_else(Local::now)
}

fn parse_usage_with_meta(
    jsonl_path: &Path,
) -> (TokenUsage, Option<String>, Option<String>, Option<DateTime<Local>>, Option<String>, HashMap<String, f64>) {
    let session_id = jsonl_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let attrs = match fs::metadata(jsonl_path) {
        Ok(a) => a,
        Err(_) => return (TokenUsage::default(), None, None, None, None, HashMap::new()),
    };
    let size = attrs.len();
    let mtime = attrs.modified().ok();
    let mtime_dt = mtime.map(datetime_of);
    let fingerprint = match mtime {
        Some(t) => format!(
            "{}:{}",
            size,
            t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
        ),
        None => format!("{}:0", size),
    };

    if let Some(cached) = cache_get(&session_id) {
        if cached.fingerprint == fingerprint {
            return (
                cached.usage,
                cached.model,
                cached.ai_title,
                mtime_dt,
                cached.cwd,
                cached.cost_by_day,
            );
        }
    }

    let content = match fs::read_to_string(jsonl_path) {
        Ok(c) => c,
        Err(_) => return (TokenUsage::default(), None, None, mtime_dt, None, HashMap::new()),
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
            Some(cc) => (
                cc.get("ephemeral_5m_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cc.get("ephemeral_1h_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            ),
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

    (usage, model, ai_title, mtime_dt, cwd, cost_by_day)
}

/// Two-phase result: Phase 1 (live + recent) followed by Phase 2 (full history).
/// We return them merged in a single pass for the TUI; the cache means re-scans
/// are nearly free.
pub fn scan(history_days: i64) -> Vec<SessionInfo> {
    let claude = claude_dir();
    let names = load_custom_names();

    let mut by_id: HashMap<String, SessionInfo> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Phase 1: live PIDs.
    let sessions_dir = claude.join("sessions");
    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let data = match fs::read_to_string(&path) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let json: Value = match serde_json::from_str(&data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let pid = json.get("pid").and_then(|v| v.as_i64()).unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0)
            }) as i32;
            if !pid_alive(pid) {
                continue;
            }
            let session_id = json
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cwd = json
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if is_junk_cwd(&cwd) {
                continue;
            }
            let status = json
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let version = json
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let started_at = json
                .get("startedAt")
                .and_then(|v| v.as_f64())
                .and_then(|ms| {
                    Local
                        .timestamp_millis_opt(ms as i64)
                        .single()
                });

            // Backfill usage from the project's JSONL if it exists.
            let project_dir = claude.join("projects").join(project_name_for(&cwd));
            let jsonl_path = project_dir.join(format!("{}.jsonl", session_id));
            let (usage, model, ai_title, _, _, cost_by_day) = if jsonl_path.exists() {
                parse_usage_with_meta(&jsonl_path)
            } else {
                (TokenUsage::default(), None, None, None, None, HashMap::new())
            };

            let cost = cost_for(&usage, model.as_deref());
            let name = names
                .get(&session_id)
                .cloned()
                .or(ai_title)
                .unwrap_or_else(|| short_path(&cwd));

            seen.insert(session_id.clone());
            by_id.insert(
                session_id.clone(),
                SessionInfo {
                    source: Source::Claude,
                    id: session_id.clone(),
                    pid,
                    name,
                    cwd,
                    status,
                    started_at,
                    last_activity_at: Some(Local::now()),
                    version,
                    model,
                    usage,
                    cost,
                    cost_by_day,
                    is_alive: true,
                },
            );
        }
    }

    // Phase 2: history scan within horizon.
    let projects_dir = claude.join("projects");
    let horizon = Local::now() - chrono::Duration::days(history_days);
    let max_bytes: u64 = 100 * 1024 * 1024;

    if let Ok(projects) = fs::read_dir(&projects_dir) {
        for project in projects.flatten() {
            let project_path = project.path();
            if !project_path.is_dir() {
                continue;
            }
            let project_name = project.file_name().to_string_lossy().to_string();
            let cwd_guess = cwd_from_project_name(&project_name);
            if is_junk_cwd(&cwd_guess) {
                continue;
            }

            let jsonls = match fs::read_dir(&project_path) {
                Ok(j) => j,
                Err(_) => continue,
            };
            for jsonl in jsonls.flatten() {
                let path = jsonl.path();
                if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                    continue;
                }
                let attrs = match fs::metadata(&path) {
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
                let session_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if seen.contains(&session_id) {
                    continue;
                }
                let (usage, model, ai_title, last_mod, cwd_from_jsonl, cost_by_day) =
                    parse_usage_with_meta(&path);
                let cwd = cwd_from_jsonl.unwrap_or(cwd_guess.clone());
                if is_junk_cwd(&cwd) {
                    continue;
                }
                let cost = cost_for(&usage, model.as_deref());
                let name = names
                    .get(&session_id)
                    .cloned()
                    .or(ai_title)
                    .unwrap_or_else(|| short_path(&cwd));

                by_id.insert(
                    session_id.clone(),
                    SessionInfo {
                        source: Source::Claude,
                        id: session_id.clone(),
                        pid: 0,
                        name,
                        cwd,
                        status: "ended".to_string(),
                        started_at: last_mod,
                        last_activity_at: Some(last_mod.unwrap_or(mtime)),
                        version: String::new(),
                        model,
                        usage,
                        cost,
                        cost_by_day,
                        is_alive: false,
                    },
                );
            }
        }
    }

    // Phase 3: OpenAI Codex sessions (read-only; Codex has no live-PID concept,
    // so these always present as historical and resume via `codex resume <id>`).
    scan_codex(history_days, &names, &mut by_id, &mut seen);

    let mut out: Vec<SessionInfo> = by_id.into_values().collect();
    sort_sessions(&mut out);
    out
}

fn project_name_for(cwd: &str) -> String {
    let s = cwd.strip_prefix('/').unwrap_or(cwd);
    format!("-{}", s.replace('/', "-"))
}

/// Extract the session UUID from a Codex rollout filename, which looks like
/// `rollout-2026-06-01T22-42-08-019e8635-bb96-7e23-9590-e551cb9e2806.jsonl`.
/// The UUID is the trailing five dash-separated groups (8-4-4-4-12).
fn codex_id_from_filename(fname: &str) -> Option<String> {
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
    (g("input_tokens"), g("cached_input_tokens"), g("output_tokens"))
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
fn scan_codex(
    history_days: i64,
    names: &HashMap<String, String>,
    by_id: &mut HashMap<String, SessionInfo>,
    seen: &mut HashSet<String>,
) {
    let sessions_dir = codex_dir().join("sessions");
    if !sessions_dir.is_dir() {
        return;
    }
    let horizon = Local::now() - chrono::Duration::days(history_days);
    let max_bytes: u64 = 100 * 1024 * 1024;

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
        let Some(id) = codex_id_from_filename(fname) else {
            continue;
        };
        if seen.contains(&id) {
            continue;
        }
        if let Some(s) = parse_codex_rollout(path, &id, names, mtime) {
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
    mtime: DateTime<Local>,
) -> Option<SessionInfo> {
    let attrs = fs::metadata(path).ok()?;
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
                if let Some(m) = payload.and_then(|p| p.get("model")).and_then(|v| v.as_str()) {
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

/// Delete JSONL files for sessions matching `predicate` and return how many
/// files were deleted. Callers should remove the corresponding entries from
/// their in-memory list separately.
pub fn delete_sessions<F>(sessions: &[SessionInfo], predicate: F) -> (Vec<String>, usize)
where
    F: Fn(&SessionInfo) -> bool,
{
    let projects_dir = claude_dir().join("projects");
    let projects = match fs::read_dir(&projects_dir) {
        Ok(p) => p.flatten().map(|e| e.path()).collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };

    let mut removed_ids: Vec<String> = Vec::new();
    let mut removed_files = 0;
    let mut cache = CACHE.lock().ok();

    for s in sessions.iter().filter(|s| predicate(s)) {
        let mut found = false;
        for proj in &projects {
            let f = proj.join(format!("{}.jsonl", s.id));
            if f.exists() {
                if fs::remove_file(&f).is_ok() {
                    removed_files += 1;
                    found = true;
                }
                break;
            }
        }
        if found {
            removed_ids.push(s.id.clone());
            if let Some(c) = cache.as_mut() {
                if let Some(map) = c.as_mut() {
                    map.remove(&s.id);
                }
            }
        }
    }

    (removed_ids, removed_files)
}

/// Cheap refresh: re-read `~/.claude/sessions/*.json` and update each session's
/// PID / status / liveness flag. Does NOT touch JSONL files — use this between
/// full scans so busy↔idle transitions show up quickly without re-parsing
/// gigabytes of conversation history.
pub fn refresh_live_status(sessions: &mut [SessionInfo]) {
    let dir = claude_dir().join("sessions");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut live: HashMap<String, (i32, String)> = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let json: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let pid = json.get("pid").and_then(|v| v.as_i64()).unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
        }) as i32;
        if !pid_alive(pid) {
            continue;
        }
        let id = json
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let status = json
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        live.insert(id, (pid, status));
    }

    for s in sessions.iter_mut() {
        match live.get(&s.id) {
            Some((pid, status)) => {
                s.pid = *pid;
                s.status = status.clone();
                s.is_alive = true;
            }
            None => {
                if s.is_alive {
                    s.is_alive = false;
                    if s.status != "ended" {
                        s.status = "ended".to_string();
                    }
                }
            }
        }
    }
}

pub fn is_junk_session(s: &SessionInfo) -> bool {
    !s.is_alive && (is_junk_cwd(&s.cwd) || s.usage.message_count == 0)
}

pub fn is_empty_session(s: &SessionInfo) -> bool {
    !s.is_alive && s.usage.message_count == 0
}

pub fn sort_sessions(s: &mut [SessionInfo]) {
    s.sort_by(|a, b| {
        match (a.is_alive, b.is_alive) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        match (a.is_recently_active(), b.is_recently_active()) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        b.last_activity_at.cmp(&a.last_activity_at)
    });
}
