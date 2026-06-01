use anyhow::{anyhow, Result};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::models::{short_path, SessionInfo};

fn find_claude() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLAUDE_BIN") {
        if !p.is_empty() {
            let pb = PathBuf::from(p);
            if pb.exists() {
                return Some(pb);
            }
        }
    }
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".claude/local/bin/claude"));
        candidates.push(home.join(".local/bin/claude"));
    }
    for c in candidates {
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

/// One-shot to `claude --print --model haiku`. Writes prompt to stdin, returns
/// the trimmed stdout. Times out after `timeout`.
fn run_haiku(prompt: &str, timeout: Duration) -> Result<String> {
    let claude = find_claude().ok_or_else(|| anyhow!("claude binary not found"))?;
    let mut child = Command::new(&claude)
        .arg("--print")
        .arg("--model")
        .arg("haiku")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(prompt.as_bytes())?;
    }
    drop(child.stdin.take());

    // Crude timeout: poll every 200ms.
    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(_status) => break,
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(anyhow!("timeout"));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    let output = child.wait_with_output()?;
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(raw)
}

/// Suggest a 1-5 word session name from a JSONL snippet. Returns None on any failure.
pub fn suggest_name(snippet: &str) -> Option<String> {
    let prompt = format!(
        "Read this Claude Code session snippet and propose a short session name \
(max 5 words, no quotes, no period). Capture the topic concretely. Reply with ONLY the name, no prose.\n\n{}",
        snippet
    );
    let raw = run_haiku(&prompt, Duration::from_secs(45)).ok()?;
    let cleaned: String = raw
        .replace(['"', '\''], "")
        .trim()
        .to_string();
    if cleaned.is_empty()
        || cleaned.contains("\n\n")
        || cleaned.chars().count() > 80
        || cleaned.to_lowercase().contains("error")
    {
        return None;
    }
    Some(cleaned)
}

/// Sample first user/assistant exchange of a JSONL — used as auto-naming context.
pub fn sample_session_text(sessions_dir: &std::path::Path, session_id: &str) -> Option<String> {
    let projects = std::fs::read_dir(sessions_dir).ok()?;
    for project in projects.flatten() {
        let path = project.path().join(format!("{}.jsonl", session_id));
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        let mut pieces = Vec::new();
        for line in content.lines() {
            if line.is_empty() {
                continue;
            }
            let obj: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let ty = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ty == "user" {
                if let Some(msg) = obj.get("message") {
                    if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
                        pieces.push(format!("USER: {}", truncate_text(s, 400)));
                    } else if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                        if let Some(first) = arr.iter().find(|o| {
                            o.get("type").and_then(|v| v.as_str()) == Some("text")
                        }) {
                            if let Some(t) = first.get("text").and_then(|v| v.as_str()) {
                                pieces.push(format!("USER: {}", truncate_text(t, 400)));
                            }
                        }
                    }
                }
            } else if ty == "assistant" {
                if let Some(msg) = obj.get("message") {
                    if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                        if let Some(first) = arr.iter().find(|o| {
                            o.get("type").and_then(|v| v.as_str()) == Some("text")
                        }) {
                            if let Some(t) = first.get("text").and_then(|v| v.as_str()) {
                                pieces.push(format!("ASSISTANT: {}", truncate_text(t, 400)));
                            }
                        }
                    }
                }
            }
            if pieces.len() >= 4 {
                break;
            }
        }
        if pieces.is_empty() {
            return None;
        }
        return Some(pieces.join("\n\n"));
    }
    None
}

fn truncate_text(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Semantic search across sessions — used as a fallback when literal filter
/// returns 0 matches. Returns the matched session id (or None).
pub fn search(query: &str, sessions: &[SessionInfo]) -> Option<String> {
    let mut compact: Vec<Value> = Vec::new();
    for s in sessions.iter().take(60) {
        compact.push(serde_json::json!({
            "id": s.id,
            "name": s.name,
            "cwd": short_path(&s.cwd),
            "messages": s.usage.message_count,
            "cost": format!("{:.4}", s.cost),
        }));
    }
    let sessions_json = serde_json::to_string(&compact).ok()?;
    let prompt = format!(
        "You are a session search assistant. Given a user query and a list of Claude Code sessions \
(id, name, cwd, messages, cost), pick the single best matching session by intent.\n\n\
Reply with ONE LINE of pure JSON only, no prose, no code fences:\n\
{{\"id\":\"<sessionId>\",\"reason\":\"<short explanation, max 60 chars>\"}}\n\n\
If nothing matches, reply: {{\"id\":null,\"reason\":\"<short explanation>\"}}\n\n\
Query: {}\n\nSessions:\n{}",
        query, sessions_json
    );
    let raw = run_haiku(&prompt, Duration::from_secs(30)).ok()?;
    let cleaned = raw.replace("```json", "").replace("```", "");
    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}')?;
    let json: Value = serde_json::from_str(&cleaned[start..=end]).ok()?;
    json.get("id").and_then(|v| v.as_str()).map(String::from)
}
