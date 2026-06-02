use anyhow::Result;
use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{cost_for, short_path, SessionInfo, Source, TokenUsage};

mod claude;
mod codex;

use claude::{parse_usage_with_meta, ParsedSession};
use codex::scan_codex;

pub use codex::codex_id_from_filename;

#[derive(Debug, Clone)]
pub(crate) struct JsonlCache {
    pub(crate) fingerprint: String,
    pub(crate) usage: TokenUsage,
    pub(crate) model: Option<String>,
    pub(crate) ai_title: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) cost_by_day: HashMap<String, f64>,
}

static CACHE: Mutex<Option<HashMap<String, JsonlCache>>> = Mutex::new(None);

pub(crate) fn cache_get(session_id: &str) -> Option<JsonlCache> {
    let mut g = CACHE.lock().ok()?;
    g.get_or_insert_with(HashMap::new).get(session_id).cloned()
}

pub(crate) fn cache_put(session_id: &str, entry: JsonlCache) {
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

pub(crate) fn is_junk_cwd(cwd: &str) -> bool {
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

pub(crate) fn datetime_of(t: SystemTime) -> DateTime<Local> {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    Local
        .timestamp_opt(d.as_secs() as i64, d.subsec_nanos())
        .single()
        .unwrap_or_else(Local::now)
}

/// Options controlling a scan (sources, horizon, size cap).
pub struct ScanOpts {
    pub history_days: i64,
    pub scan_claude: bool,
    pub scan_codex: bool,
    pub max_jsonl_bytes: u64,
}

/// Two-phase result: Phase 1 (live + recent) followed by Phase 2 (full history),
/// plus Phase 3 (Codex). We return them merged in a single pass for the TUI;
/// the cache means re-scans are nearly free.
pub fn scan(opts: &ScanOpts) -> Vec<SessionInfo> {
    let history_days = opts.history_days;
    let claude = claude_dir();
    let names = load_custom_names();

    let mut by_id: HashMap<String, SessionInfo> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Phase 1: live PIDs (Claude only). An empty path when disabled makes the
    // read_dir below a no-op.
    let sessions_dir = if opts.scan_claude {
        claude.join("sessions")
    } else {
        PathBuf::new()
    };
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
                .and_then(|ms| Local.timestamp_millis_opt(ms as i64).single());

            // Backfill usage from the project's JSONL if it exists.
            let project_dir = claude.join("projects").join(project_name_for(&cwd));
            let jsonl_path = project_dir.join(format!("{}.jsonl", session_id));
            let ParsedSession {
                usage,
                model,
                ai_title,
                cost_by_day,
                ..
            } = if jsonl_path.exists() {
                parse_usage_with_meta(&jsonl_path)
            } else {
                ParsedSession::default()
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

    // Phase 2: history scan within horizon (Claude only).
    let projects_dir = if opts.scan_claude {
        claude.join("projects")
    } else {
        PathBuf::new()
    };
    let horizon = Local::now() - chrono::Duration::days(history_days);
    let max_bytes: u64 = opts.max_jsonl_bytes;

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
                let ParsedSession {
                    usage,
                    model,
                    ai_title,
                    last_modified: last_mod,
                    cwd: cwd_from_jsonl,
                    cost_by_day,
                } = parse_usage_with_meta(&path);
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
    if opts.scan_codex {
        scan_codex(opts, &names, &mut by_id, &mut seen);
    }

    let mut out: Vec<SessionInfo> = by_id.into_values().collect();
    sort_sessions(&mut out);
    out
}

pub(crate) fn project_name_for(cwd: &str) -> String {
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
