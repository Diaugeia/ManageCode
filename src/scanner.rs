use anyhow::Result;
use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{cost_for, short_path, SessionInfo, TokenUsage};

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
        let cc_t = u
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        usage.total_input += in_t;
        usage.total_output += out_t;
        usage.cache_read += cr_t;
        usage.cache_creation += cc_t;
        if obj.get("isSidechain").and_then(|v| v.as_bool()) != Some(true) {
            usage.message_count += 1;
        }
        let msg_model = msg.get("model").and_then(|v| v.as_str()).map(String::from);
        if let Some(m) = &msg_model {
            model = Some(m.clone());
        }

        // Bucket this message's cost by the day it was sent (local time).
        let (pi, po, pcr, pcw) =
            crate::models::pricing_for(msg_model.as_deref().or(model.as_deref()));
        let msg_cost = (in_t as f64) / 1_000_000.0 * pi
            + (out_t as f64) / 1_000_000.0 * po
            + (cr_t as f64) / 1_000_000.0 * pcr
            + (cc_t as f64) / 1_000_000.0 * pcw;
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

    let mut out: Vec<SessionInfo> = by_id.into_values().collect();
    sort_sessions(&mut out);
    out
}

fn project_name_for(cwd: &str) -> String {
    let s = cwd.strip_prefix('/').unwrap_or(cwd);
    format!("-{}", s.replace('/', "-"))
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
