//! Turning a launch intent (resume / new claude / shell / custom) into a live
//! embedded terminal. When tmux is available the command runs inside a detached
//! tmux session that the PTY `tmux attach`es to (so it survives detach/quit);
//! otherwise the command runs directly in the PTY.

use crate::app::{App, PendingExec};
use crate::models::Source;
use crate::pty::TerminalSpec;
use crate::tmux;

/// Locate the `claude` binary: `$CLAUDE_BIN`, then well-known install paths,
/// then fall back to a bare `claude` on `$PATH`.
fn find_claude_binary() -> Option<String> {
    if let Ok(p) = std::env::var("CLAUDE_BIN") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    let mut candidates: Vec<String> = vec![
        "/opt/homebrew/bin/claude".into(),
        "/usr/local/bin/claude".into(),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".claude/local/bin/claude").to_string_lossy().to_string());
        candidates.push(home.join(".local/bin/claude").to_string_lossy().to_string());
    }
    for c in candidates {
        if std::fs::metadata(&c).is_ok() {
            return Some(c);
        }
    }
    // Fall back to PATH lookup.
    Some("claude".into())
}

/// Locate the `codex` binary: `$CODEX_BIN`, then well-known install paths,
/// then fall back to a bare `codex` on `$PATH`.
fn find_codex_binary() -> Option<String> {
    if let Ok(p) = std::env::var("CODEX_BIN") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    let mut candidates: Vec<String> =
        vec!["/opt/homebrew/bin/codex".into(), "/usr/local/bin/codex".into()];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/codex").to_string_lossy().to_string());
    }
    for c in candidates {
        if std::fs::metadata(&c).is_ok() {
            return Some(c);
        }
    }
    Some("codex".into())
}

/// Open an embedded terminal for a launch intent. When tmux is available the
/// command runs inside a detached tmux session that we `tmux attach` to in the
/// PTY (so it persists across detach/quit); otherwise the command runs directly
/// in the PTY.
pub fn open_terminal_for(app: &mut App, pending: PendingExec) {
    let use_tmux = app.config.prefer_tmux && tmux::available() && !tmux::inside_tmux();

    // Each arm resolves only the binary it needs (a shell needs none; resume
    // uses the CLI that owns the session; new-claude/custom need claude).
    let spec = match pending {
        PendingExec::NewShell { cwd } => {
            if use_tmux {
                let name = tmux::new_shell_name();
                tmux::ensure_session_shell(&name, &cwd);
                attach_spec(&name, &cwd, "shell")
            } else {
                TerminalSpec {
                    cwd,
                    argv: vec![],
                    title: "shell".into(),
                }
            }
        }
        PendingExec::Resume {
            id,
            cwd,
            is_alive,
            source,
        } => {
            // Resume with the CLI that owns the session: `claude --resume <id>`
            // or `codex resume <id>`.
            let resolved = match source {
                Source::Claude => find_claude_binary()
                    .map(|b| (b, vec!["--resume".to_string(), id.clone()], "claude")),
                Source::Codex => {
                    find_codex_binary().map(|b| (b, vec!["resume".to_string(), id.clone()], "codex"))
                }
            };
            let Some((bin, args, title)) = resolved else {
                app.flash(match source {
                    Source::Claude => "claude binary not found",
                    Source::Codex => "codex binary not found",
                });
                return;
            };
            if use_tmux {
                let name = tmux::resume_name(&id);
                let mut parts: Vec<&str> = Vec::with_capacity(1 + args.len());
                parts.push(bin.as_str());
                for a in &args {
                    parts.push(a.as_str());
                }
                let cmd = tmux::join_command(&parts);
                // A dead (historical) session must actually re-run resume. Kill
                // any stale backing session first so we don't re-attach to an
                // exited/empty pane (which can linger under a `remain-on-exit`
                // tmux config). A live session keeps its running process:
                // ensure_session is then a no-op and we simply re-attach.
                if !is_alive {
                    tmux::kill_session(&name);
                }
                tmux::ensure_session(&name, &cwd, &cmd);
                attach_spec(&name, &cwd, title)
            } else {
                let mut argv = vec![bin];
                argv.extend(args);
                TerminalSpec {
                    cwd,
                    argv,
                    title: title.to_string(),
                }
            }
        }
        PendingExec::NewClaude { cwd } => {
            let Some(claude) = find_claude_binary() else {
                app.flash("claude binary not found");
                return;
            };
            if use_tmux {
                let name = tmux::new_claude_name();
                let cmd = tmux::sh_quote(&claude);
                tmux::ensure_session(&name, &cwd, &cmd);
                attach_spec(&name, &cwd, "claude")
            } else {
                TerminalSpec {
                    cwd,
                    argv: vec![claude],
                    title: "claude".into(),
                }
            }
        }
        PendingExec::Custom { cwd, args } => {
            let Some(claude) = find_claude_binary() else {
                app.flash("claude binary not found");
                return;
            };
            if use_tmux {
                let name = tmux::new_claude_name();
                let mut parts: Vec<&str> = Vec::with_capacity(1 + args.len());
                parts.push(&claude);
                for a in &args {
                    parts.push(a);
                }
                let cmd = tmux::join_command(&parts);
                tmux::ensure_session(&name, &cwd, &cmd);
                attach_spec(&name, &cwd, "claude")
            } else {
                let mut argv = vec![claude];
                argv.extend(args);
                TerminalSpec {
                    cwd,
                    argv,
                    title: "claude".into(),
                }
            }
        }
    };
    app.request_terminal(spec);
}

/// A TerminalSpec that attaches to an existing tmux session inside the PTY.
fn attach_spec(name: &str, cwd: &str, title: &str) -> TerminalSpec {
    TerminalSpec {
        cwd: cwd.to_string(),
        argv: vec![
            "tmux".into(),
            "attach-session".into(),
            "-t".into(),
            name.to_string(),
        ],
        title: title.to_string(),
    }
}
