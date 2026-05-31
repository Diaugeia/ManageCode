use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// All tmux sessions managed by ManageCode are prefixed with this string,
/// so we can find them with a single `list-sessions` call and not collide
/// with the user's own tmux sessions.
pub const PREFIX: &str = "mc-";

/// Is `tmux` present on $PATH?
pub fn available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Are we currently running inside a tmux client? If yes, we should
/// fall back to direct exec rather than try to nest tmux.
pub fn inside_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// All `mc-*` tmux sessions currently alive on this user's tmux server.
pub fn list_managed() -> Vec<String> {
    let out = Command::new("tmux")
        .args(["list-sessions", "-F", "#S"])
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| s.starts_with(PREFIX))
        .collect()
}

pub fn list_managed_set() -> HashSet<String> {
    list_managed().into_iter().collect()
}

pub fn has_session(name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn kill_session(name: &str) -> bool {
    Command::new("tmux")
        .args(["kill-session", "-t", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Tmux session name for a known Claude session id. Truncated to keep names
/// short and readable in `tmux ls`.
pub fn resume_name(claude_session_id: &str) -> String {
    let short = &claude_session_id[..claude_session_id.len().min(12)];
    format!("{}{}", PREFIX, short)
}

pub fn new_claude_name() -> String {
    format!("{}new-{}", PREFIX, short_rand_hex())
}

pub fn new_shell_name() -> String {
    format!("{}sh-{}", PREFIX, short_rand_hex())
}

fn short_rand_hex() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos;
    let mut s = String::with_capacity(8);
    for _ in 0..8 {
        let d = (x & 0xf) as u32;
        s.push(char::from_digit(d, 16).unwrap_or('0'));
        x >>= 4;
    }
    s
}

/// Create a detached tmux session that will run `command` (a sh-parsed line)
/// in `cwd`. No-op if the session already exists.
pub fn ensure_session(name: &str, cwd: &str, command: &str) -> bool {
    if has_session(name) {
        return true;
    }
    Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-c", cwd, command])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Create a detached tmux session in `cwd` that runs the user's default
/// shell. No-op if the session already exists.
pub fn ensure_session_shell(name: &str, cwd: &str) -> bool {
    if has_session(name) {
        return true;
    }
    Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-c", cwd])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// The old full-screen `attach` helper was removed — sessions are now attached
// to inside an embedded PTY pane (see main::attach_spec).

/// Shell-escape a single argument so it survives one round of sh parsing
/// (what tmux applies to the trailing command string).
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | ',' | '=')
        })
    {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

pub fn join_command(args: &[&str]) -> String {
    args.iter()
        .map(|a| sh_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}
