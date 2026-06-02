//! Turning a launch intent (resume / new claude / shell / custom) into a live
//! embedded terminal. When tmux is available the command runs inside a detached
//! tmux session that the PTY `tmux attach`es to (so it survives detach/quit);
//! otherwise the command runs directly in the PTY.

use crate::app::{App, PendingExec};
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

/// Open an embedded terminal for a launch intent. When tmux is available the
/// command runs inside a detached tmux session that we `tmux attach` to in the
/// PTY (so it persists across detach/quit); otherwise the command runs directly
/// in the PTY.
pub fn open_terminal_for(app: &mut App, pending: PendingExec) {
    let use_tmux = tmux::available() && !tmux::inside_tmux();

    // A plain shell needs no claude binary, so handle it up front; the remaining
    // arms all require claude, so resolve and check it once for them.
    if let PendingExec::NewShell { cwd } = pending {
        let spec = if use_tmux {
            let name = tmux::new_shell_name();
            tmux::ensure_session_shell(&name, &cwd);
            attach_spec(&name, &cwd, "shell")
        } else {
            TerminalSpec {
                cwd,
                argv: vec![],
                title: "shell".into(),
            }
        };
        app.request_terminal(spec);
        return;
    }

    let Some(claude) = find_claude_binary() else {
        app.flash("claude binary not found");
        return;
    };
    let spec = match pending {
        PendingExec::Resume { id, cwd, is_alive } => {
            if use_tmux {
                let name = tmux::resume_name(&id);
                let cmd = tmux::join_command(&[&claude, "--resume", &id]);
                // A dead (historical) session must actually re-run
                // `claude --resume <id>`. Kill any stale backing session first
                // so we don't re-attach to an exited/empty pane (which can
                // linger under a `remain-on-exit` tmux config). A live session
                // keeps its running process: ensure_session is then a no-op and
                // we simply re-attach.
                if !is_alive {
                    tmux::kill_session(&name);
                }
                tmux::ensure_session(&name, &cwd, &cmd);
                attach_spec(&name, &cwd, "claude")
            } else {
                TerminalSpec {
                    cwd,
                    argv: vec![claude, "--resume".into(), id],
                    title: "claude".into(),
                }
            }
        }
        PendingExec::NewClaude { cwd } => {
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
        // Handled by the early return above.
        PendingExec::NewShell { .. } => unreachable!(),
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
