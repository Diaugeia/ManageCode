use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::ai;
use crate::config::Config;
use crate::models::SessionInfo;
use crate::notifications::Notifier;
use crate::pty::{TermSession, TerminalSpec};
use crate::scanner;
use crate::tmux;
use crate::watcher;

/// One row in the visible list — either a project header or a session row.
#[derive(Clone)]
pub enum Row {
    Header {
        cwd: String,
        total: usize,
        alive: usize,
        collapsed: bool,
    },
    Session(usize), // index into App.sessions
}

/// Top-level UI mode.
pub enum Mode {
    Browse,
    Filter,
    Rename,
    Confirm(ConfirmAction),
    Help,
    Launch(LaunchForm),
    /// An embedded terminal pane is active and receiving keystrokes.
    Terminal,
    /// The settings overlay is open.
    Settings,
    /// The cost-summary overlay is open.
    CostSummary,
}

#[derive(Clone)]
pub struct LaunchForm {
    pub cwd: String,
    pub resume_id: Option<String>,
    pub field: usize,
    pub model: LaunchModel,
    pub dangerously_skip_permissions: bool,
    pub sandbox: bool,
    pub verbose: bool,
    pub add_dir: String,
    /// Distinct recent cwds for quick selection on the cwd field.
    pub recent_dirs: Vec<String>,
    /// Index into recent_dirs for Left/Right cycling.
    pub dir_idx: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum LaunchModel {
    Auto,
    Opus,
    Sonnet,
    Haiku,
}

impl LaunchModel {
    pub fn label(&self) -> &'static str {
        match self {
            LaunchModel::Auto => "auto",
            LaunchModel::Opus => "opus",
            LaunchModel::Sonnet => "sonnet",
            LaunchModel::Haiku => "haiku",
        }
    }
    pub fn flag(&self) -> Option<&'static str> {
        match self {
            LaunchModel::Auto => None,
            LaunchModel::Opus => Some("opus"),
            LaunchModel::Sonnet => Some("sonnet"),
            LaunchModel::Haiku => Some("haiku"),
        }
    }
    pub fn cycle(&self) -> Self {
        match self {
            LaunchModel::Auto => LaunchModel::Opus,
            LaunchModel::Opus => LaunchModel::Sonnet,
            LaunchModel::Sonnet => LaunchModel::Haiku,
            LaunchModel::Haiku => LaunchModel::Auto,
        }
    }
}

impl LaunchForm {
    pub fn new(cwd: String, resume_id: Option<String>, recent_dirs: Vec<String>) -> Self {
        LaunchForm {
            cwd,
            resume_id,
            field: 0,
            model: LaunchModel::Auto,
            dangerously_skip_permissions: false,
            sandbox: false,
            verbose: false,
            add_dir: String::new(),
            recent_dirs,
            dir_idx: 0,
        }
    }

    pub const FIELD_COUNT: usize = 6;

    /// Cycle the cwd through the recent-dirs list (Left/Right on the cwd field).
    pub fn cycle_dir(&mut self, forward: bool) {
        if self.recent_dirs.is_empty() {
            return;
        }
        let n = self.recent_dirs.len();
        self.dir_idx = if forward {
            (self.dir_idx + 1) % n
        } else {
            (self.dir_idx + n - 1) % n
        };
        self.cwd = self.recent_dirs[self.dir_idx].clone();
    }

    pub fn field_label(&self, i: usize) -> &'static str {
        match i {
            0 => "cwd",
            1 => "model",
            2 => "--dangerously-skip-permissions",
            3 => "--sandbox",
            4 => "--verbose",
            5 => "--add-dir",
            _ => "",
        }
    }

    pub fn field_value(&self, i: usize) -> String {
        match i {
            0 => crate::models::short_path(&self.cwd),
            1 => self.model.label().to_string(),
            2 => bool_label(self.dangerously_skip_permissions),
            3 => bool_label(self.sandbox),
            4 => bool_label(self.verbose),
            5 => {
                if self.add_dir.is_empty() {
                    "—".into()
                } else {
                    self.add_dir.clone()
                }
            }
            _ => String::new(),
        }
    }

    pub fn toggle_field(&mut self) {
        match self.field {
            1 => self.model = self.model.cycle(),
            2 => self.dangerously_skip_permissions = !self.dangerously_skip_permissions,
            3 => self.sandbox = !self.sandbox,
            4 => self.verbose = !self.verbose,
            _ => {}
        }
    }

    pub fn args(&self) -> Vec<String> {
        let mut a = Vec::new();
        if let Some(id) = &self.resume_id {
            a.push("--resume".into());
            a.push(id.clone());
        }
        if let Some(m) = self.model.flag() {
            a.push("--model".into());
            a.push(m.into());
        }
        if self.dangerously_skip_permissions {
            a.push("--dangerously-skip-permissions".into());
        }
        if self.sandbox {
            a.push("--sandbox".into());
        }
        if self.verbose {
            a.push("--verbose".into());
        }
        if !self.add_dir.is_empty() {
            a.push("--add-dir".into());
            a.push(self.add_dir.clone());
        }
        a
    }
}

fn bool_label(b: bool) -> String {
    if b { "[x]".into() } else { "[ ]".into() }
}

#[derive(Clone)]
pub enum ConfirmAction {
    DeleteJunk,
    DeleteEmpty,
    KillTmux(String, String), // (tmux session name, claude session id) — for the toast
}

impl ConfirmAction {
    pub fn prompt(&self) -> &'static str {
        match self {
            ConfirmAction::DeleteJunk => "Delete junk sessions? (tmp / no messages)",
            ConfirmAction::DeleteEmpty => "Delete sessions with no messages?",
            ConfirmAction::KillTmux(_, _) => {
                "Kill the background tmux session? (forces claude to exit)"
            }
        }
    }
}

pub struct App {
    pub sessions: Vec<SessionInfo>,
    pub selected: usize,
    pub filter: String,
    pub rename_buf: String,
    pub mode: Mode,
    pub history_days: i64,
    pub last_scan: Instant,
    pub scanning: bool,
    pub spinner_phase: usize,
    pub message: Option<(String, Instant)>,
    pub custom_names: HashMap<String, String>,
    pub group_by_directory: bool,
    pub collapsed_groups: HashSet<String>,
    pub ai_running: bool,
    pub auto_naming: bool,
    pub auto_name_progress: (usize, usize), // (done, total)
    pub notifier: Notifier,
    pub watcher_active: bool,
    pub tmux_available: bool,
    pub tmux_backed: HashSet<String>,
    /// The live embedded terminal, if one is open.
    pub term: Option<TermSession>,
    /// A terminal requested but not yet spawned (spawned by the run loop once
    /// the pane size is known).
    pub pending_terminal: Option<TerminalSpec>,
    /// Inner content rect of the terminal pane as (x, y, cols, rows), written by
    /// the renderer each frame; used to resize the PTY and map mouse coordinates.
    pub term_area: Cell<(u16, u16, u16, u16)>,
    /// User configuration (escape prefix, etc.).
    pub config: Config,
    /// Editing buffer for the settings overlay.
    pub settings_input: String,
    /// Budget editing buffer for the settings overlay (USD, "" = off).
    pub settings_budget_input: String,
    /// Which settings field is focused (0 = prefix, 1 = daily budget).
    pub settings_field: usize,
    /// Whether we've already alerted that today's spend crossed the budget.
    budget_alerted: bool,
    last_tmux_refresh: Instant,
    last_live_sweep: Instant,
    dirty_since: Option<Instant>,
    rx: mpsc::Receiver<Vec<SessionInfo>>,
    tx: mpsc::Sender<Vec<SessionInfo>>,
    ai_rx: mpsc::Receiver<AiEvent>,
    ai_tx: mpsc::Sender<AiEvent>,
    watch_rx: mpsc::Receiver<()>,
}

pub enum AiEvent {
    SearchHit(Option<String>),                    // matched session id
    NameSuggestion { session_id: String, name: String },
    AutoNameDone,
}

#[derive(Clone)]
pub enum PendingExec {
    Resume { id: String, cwd: String },
    NewClaude { cwd: String },
    NewShell { cwd: String },
    Custom { cwd: String, args: Vec<String> },
}

impl App {
    pub fn new(history_days: i64) -> Self {
        let (tx, rx) = mpsc::channel();
        let (ai_tx, ai_rx) = mpsc::channel();
        let (watch_tx, watch_rx) = mpsc::channel();
        let watcher_active = watcher::spawn(watch_tx);
        let mut app = App {
            sessions: Vec::new(),
            selected: 0,
            filter: String::new(),
            rename_buf: String::new(),
            mode: Mode::Browse,
            history_days,
            last_scan: Instant::now() - Duration::from_secs(60),
            scanning: false,
            spinner_phase: 0,
            message: None,
            custom_names: scanner::load_custom_names(),
            group_by_directory: true,
            collapsed_groups: HashSet::new(),
            ai_running: false,
            auto_naming: false,
            auto_name_progress: (0, 0),
            notifier: Notifier::new(true),
            watcher_active,
            tmux_available: tmux::available(),
            tmux_backed: HashSet::new(),
            term: None,
            pending_terminal: None,
            term_area: Cell::new((0, 0, 80, 24)),
            config: crate::config::load(),
            settings_input: String::new(),
            settings_budget_input: String::new(),
            settings_field: 0,
            budget_alerted: false,
            last_tmux_refresh: Instant::now() - Duration::from_secs(60),
            last_live_sweep: Instant::now() - Duration::from_secs(60),
            dirty_since: None,
            rx,
            tx,
            ai_rx,
            ai_tx,
            watch_rx,
        };
        app.kick_scan();
        app
    }

    pub fn kick_scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        let tx = self.tx.clone();
        let days = self.history_days;
        thread::spawn(move || {
            let result = scanner::scan(days);
            let _ = tx.send(result);
        });
    }

    pub fn tick(&mut self) {
        self.spinner_phase = (self.spinner_phase + 1) % 8;
        if let Ok(result) = self.rx.try_recv() {
            self.sessions = result;
            self.scanning = false;
            self.last_scan = Instant::now();
            self.notifier.observe(&self.sessions);
            self.clamp_selection();
        }
        // Drain AI events that arrived.
        while let Ok(ev) = self.ai_rx.try_recv() {
            self.handle_ai_event(ev);
        }
        // Drain file-watcher events — coalesce into a single dirty flag.
        let mut got_event = false;
        while let Ok(()) = self.watch_rx.try_recv() {
            got_event = true;
        }
        if got_event {
            self.dirty_since = Some(Instant::now());
        }
        // Debounced event-driven scan: ~180ms after the last event, kick a scan.
        if let Some(t) = self.dirty_since {
            if t.elapsed() >= Duration::from_millis(180) && !self.scanning {
                self.kick_scan();
                self.dirty_since = None;
            }
        }
        // Lightweight PID/status sweep every ~1.5s — picks up busy↔idle quickly
        // without touching JSONL files.
        if self.last_live_sweep.elapsed() >= Duration::from_millis(1500) {
            scanner::refresh_live_status(&mut self.sessions);
            self.notifier.observe(&self.sessions);
            self.last_live_sweep = Instant::now();
        }
        // Refresh the set of background tmux sessions every ~2s.
        if self.tmux_available && self.last_tmux_refresh.elapsed() >= Duration::from_secs(2) {
            self.refresh_tmux_backed();
            self.last_tmux_refresh = Instant::now();
        }
        // Fallback full scan. Faster when watcher couldn't attach.
        let fallback = if self.watcher_active {
            Duration::from_secs(30)
        } else {
            Duration::from_secs(5)
        };
        if self.last_scan.elapsed() >= fallback && !self.scanning {
            self.kick_scan();
        }
        self.check_budget();
        if let Some((_, when)) = self.message {
            if when.elapsed() > Duration::from_secs(3) {
                self.message = None;
            }
        }
    }

    fn handle_ai_event(&mut self, ev: AiEvent) {
        match ev {
            AiEvent::SearchHit(opt) => {
                self.ai_running = false;
                match opt {
                    Some(id) => {
                        let vis = self.visible_session_indices();
                        if let Some(pos) = vis.iter().position(|i| self.sessions[*i].id == id) {
                            self.selected = pos;
                            self.flash("AI: found a match");
                        } else if let Some(pos_all) =
                            self.sessions.iter().position(|s| s.id == id)
                        {
                            // The match might be hidden by filter or collapsed group; expose it.
                            let cwd = self.sessions[pos_all].cwd.clone();
                            self.collapsed_groups.remove(&cwd);
                            self.filter.clear();
                            let vis = self.visible_session_indices();
                            if let Some(p) = vis.iter().position(|i| self.sessions[*i].id == id) {
                                self.selected = p;
                            }
                            self.flash("AI: cleared filter to reveal match");
                        } else {
                            self.flash("AI: match not in current list");
                        }
                    }
                    None => self.flash("AI: no match"),
                }
            }
            AiEvent::NameSuggestion { session_id, name } => {
                self.custom_names.insert(session_id.clone(), name.clone());
                let _ = scanner::save_custom_names(&self.custom_names);
                if let Some(idx) = self.sessions.iter().position(|s| s.id == session_id) {
                    self.sessions[idx].name = name;
                }
                self.auto_name_progress.0 += 1;
            }
            AiEvent::AutoNameDone => {
                self.auto_naming = false;
                let n = self.auto_name_progress.0;
                self.flash(format!("auto-named {} session(s)", n));
            }
        }
    }

    pub fn kick_ai_search(&mut self, query: String) {
        if self.ai_running {
            return;
        }
        self.ai_running = true;
        let tx = self.ai_tx.clone();
        let sessions = self.sessions.clone();
        std::thread::spawn(move || {
            let id = ai::search(&query, &sessions);
            let _ = tx.send(AiEvent::SearchHit(id));
        });
    }

    pub fn kick_auto_name(&mut self) {
        if self.auto_naming {
            return;
        }
        // Pick candidates: not custom-named, not ai-titled (i.e., name == short_path(cwd)),
        // and has actual conversation.
        use crate::models::short_path;
        let mut cands: Vec<(String, String)> = Vec::new(); // (id, snippet later)
        for s in &self.sessions {
            if s.usage.message_count == 0 {
                continue;
            }
            if self.custom_names.contains_key(&s.id) {
                continue;
            }
            if s.name == short_path(&s.cwd) {
                cands.push((s.id.clone(), s.id.clone()));
            }
            if cands.len() >= 12 {
                break;
            }
        }
        if cands.is_empty() {
            self.flash("nothing to auto-name");
            return;
        }
        self.auto_naming = true;
        self.auto_name_progress = (0, cands.len());
        let tx = self.ai_tx.clone();
        let projects_dir = scanner::claude_dir().join("projects");
        std::thread::spawn(move || {
            for (id, _) in cands {
                let snippet = match ai::sample_session_text(&projects_dir, &id) {
                    Some(s) => s,
                    None => continue,
                };
                if let Some(name) = ai::suggest_name(&snippet) {
                    let _ = tx.send(AiEvent::NameSuggestion { session_id: id, name });
                }
            }
            let _ = tx.send(AiEvent::AutoNameDone);
        });
    }

    /// All sessions that pass the literal filter, in display order.
    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.sessions.len()).collect();
        }
        let q = self.filter.to_ascii_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.name.to_ascii_lowercase().contains(&q)
                    || s.cwd.to_ascii_lowercase().contains(&q)
                    || s.id.to_ascii_lowercase().contains(&q)
                    || s.model
                        .as_deref()
                        .map(|m| m.to_ascii_lowercase().contains(&q))
                        .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Sessions visible to the user — same as filtered, minus any inside a
    /// collapsed group. This is what `selected` indexes into.
    pub fn visible_session_indices(&self) -> Vec<usize> {
        let filtered = self.filtered_indices();
        if !self.group_by_directory || self.collapsed_groups.is_empty() {
            return filtered;
        }
        filtered
            .into_iter()
            .filter(|&i| !self.collapsed_groups.contains(&self.sessions[i].cwd))
            .collect()
    }

    /// Headers + sessions woven together in display order. Used by the renderer.
    pub fn visible_rows(&self) -> Vec<Row> {
        let filtered = self.filtered_indices();
        if !self.group_by_directory {
            return filtered.into_iter().map(Row::Session).collect();
        }

        let mut out: Vec<Row> = Vec::new();
        let mut last_cwd: Option<String> = None;

        for &idx in &filtered {
            let s = &self.sessions[idx];
            if last_cwd.as_deref() != Some(&s.cwd) {
                last_cwd = Some(s.cwd.clone());
                let total = filtered
                    .iter()
                    .filter(|&&j| self.sessions[j].cwd == s.cwd)
                    .count();
                let alive = filtered
                    .iter()
                    .filter(|&&j| self.sessions[j].cwd == s.cwd && self.sessions[j].is_alive)
                    .count();
                out.push(Row::Header {
                    cwd: s.cwd.clone(),
                    total,
                    alive,
                    collapsed: self.collapsed_groups.contains(&s.cwd),
                });
            }
            if !self.collapsed_groups.contains(&s.cwd) {
                out.push(Row::Session(idx));
            }
        }
        out
    }

    pub fn selected_session(&self) -> Option<&SessionInfo> {
        let idxs = self.visible_session_indices();
        idxs.get(self.selected).and_then(|i| self.sessions.get(*i))
    }

    pub fn toggle_group_of_selection(&mut self) {
        if let Some(s) = self.selected_session() {
            let cwd = s.cwd.clone();
            if self.collapsed_groups.contains(&cwd) {
                self.collapsed_groups.remove(&cwd);
            } else {
                self.collapsed_groups.insert(cwd);
                // Move selection to a still-visible session.
                self.clamp_selection();
            }
        }
    }

    pub fn collapse_all_inactive(&mut self) {
        let mut by_cwd: HashMap<String, (bool, usize)> = HashMap::new();
        for s in &self.sessions {
            let e = by_cwd.entry(s.cwd.clone()).or_insert((false, 0));
            if s.is_recently_active() {
                e.0 = true;
            }
            e.1 += 1;
        }
        for (cwd, (has_active, _)) in by_cwd {
            if !has_active {
                self.collapsed_groups.insert(cwd);
            }
        }
        self.clamp_selection();
    }

    pub fn clamp_selection(&mut self) {
        let len = self.visible_session_indices().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        if self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.visible_session_indices().len();
        if len == 0 {
            return;
        }
        let cur = self.selected as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.selected = next;
    }

    pub fn flash(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), Instant::now()));
    }

    pub fn perform_confirm(&mut self) {
        if let Mode::Confirm(action) = &self.mode {
            let action = action.clone();
            match action {
                ConfirmAction::DeleteJunk => {
                    let (ids, n) = scanner::delete_sessions(&self.sessions, scanner::is_junk_session);
                    self.sessions.retain(|s| !ids.contains(&s.id));
                    self.clamp_selection();
                    self.flash(format!("deleted {} junk session(s)", n));
                }
                ConfirmAction::DeleteEmpty => {
                    let (ids, n) = scanner::delete_sessions(&self.sessions, scanner::is_empty_session);
                    self.sessions.retain(|s| !ids.contains(&s.id));
                    self.clamp_selection();
                    self.flash(format!("deleted {} empty session(s)", n));
                }
                ConfirmAction::KillTmux(tmux_name, sid) => {
                    let ok = tmux::kill_session(&tmux_name);
                    if ok {
                        self.tmux_backed.remove(&sid);
                        self.flash(format!("killed tmux session {}", tmux_name));
                    } else {
                        self.flash(format!("failed to kill {}", tmux_name));
                    }
                }
            }
        }
        self.mode = Mode::Browse;
    }

    /// Reconcile our `tmux_backed` set with what tmux actually has alive.
    fn refresh_tmux_backed(&mut self) {
        let alive = tmux::list_managed_set();
        // Keep only Claude session IDs whose corresponding tmux session is alive.
        self.tmux_backed.clear();
        for s in &self.sessions {
            let name = tmux::resume_name(&s.id);
            if alive.contains(&name) {
                self.tmux_backed.insert(s.id.clone());
            }
        }
    }

    pub fn ask_kill_tmux(&mut self) {
        let Some(s) = self.selected_session() else {
            self.flash("no selection");
            return;
        };
        if !self.tmux_backed.contains(&s.id) {
            self.flash("no tmux session for this entry");
            return;
        }
        let name = tmux::resume_name(&s.id);
        let id = s.id.clone();
        self.mode = Mode::Confirm(ConfirmAction::KillTmux(name, id));
    }

    /// Queue an embedded terminal to open; the run loop spawns it once it knows
    /// the pane size. Focuses the terminal pane immediately.
    pub fn request_terminal(&mut self, spec: TerminalSpec) {
        self.close_terminal();
        self.pending_terminal = Some(spec);
        self.mode = Mode::Terminal;
    }

    /// Tear down the embedded terminal (does not kill background tmux sessions)
    /// and return to Browse.
    pub fn close_terminal(&mut self) {
        self.term = None;
        self.pending_terminal = None;
        if matches!(self.mode, Mode::Terminal) {
            self.mode = Mode::Browse;
        }
    }

    /// Move focus from the terminal pane back to the sidebar without tearing the
    /// terminal down — it keeps running and stays visible on the right.
    pub fn blur_terminal(&mut self) {
        if matches!(self.mode, Mode::Terminal) {
            self.mode = Mode::Browse;
        }
    }

    /// Give keyboard focus to the (already open) terminal pane.
    pub fn focus_terminal(&mut self) {
        if self.has_terminal() {
            self.mode = Mode::Terminal;
        }
    }

    /// Is a terminal pane currently on screen (open or about to open)?
    pub fn has_terminal(&self) -> bool {
        self.term.is_some() || self.pending_terminal.is_some()
    }

    /// Distinct session cwds, most-recently-active first (sessions are already
    /// sorted by recency), for quick selection in the launch form.
    pub fn recent_dirs(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for s in &self.sessions {
            if !s.cwd.is_empty() && seen.insert(s.cwd.clone()) {
                out.push(s.cwd.clone());
            }
        }
        out
    }

    /// Open the settings overlay, seeding the editor with the current prefix.
    pub fn open_settings(&mut self) {
        self.settings_input = self.config.escape_prefix.label();
        self.settings_budget_input = self
            .config
            .daily_budget_usd
            .map(|v| format!("{v}"))
            .unwrap_or_default();
        self.settings_field = 0;
        self.mode = Mode::Settings;
    }

    pub fn tmux_count(&self) -> usize {
        self.tmux_backed.len()
    }

    pub fn total_cost(&self) -> f64 {
        self.sessions.iter().map(|s| s.cost).sum()
    }

    /// Total spend across all sessions for today (local calendar day).
    pub fn today_cost(&self) -> f64 {
        self.sessions.iter().map(|s| s.cost_today()).sum()
    }

    /// Fire a one-shot desktop alert when today's spend crosses the budget.
    fn check_budget(&mut self) {
        let Some(limit) = self.config.daily_budget_usd else {
            return;
        };
        if limit <= 0.0 {
            return;
        }
        if self.today_cost() >= limit {
            if !self.budget_alerted {
                self.notifier.notify_text(&format!(
                    "daily spend ${:.2} reached your budget ${:.2}",
                    self.today_cost(),
                    limit
                ));
                self.budget_alerted = true;
            }
        } else {
            self.budget_alerted = false;
        }
    }

    pub fn active_count(&self) -> usize {
        self.sessions.iter().filter(|s| s.is_alive).count()
    }

    pub fn rename_selected(&mut self) {
        if let Some(s) = self.selected_session() {
            let id = s.id.clone();
            let new = self.rename_buf.trim().to_string();
            if new.is_empty() {
                self.custom_names.remove(&id);
            } else {
                self.custom_names.insert(id.clone(), new.clone());
            }
            let _ = scanner::save_custom_names(&self.custom_names);
            if let Some(idx) = self.sessions.iter().position(|s| s.id == id) {
                self.sessions[idx].name = if new.is_empty() {
                    crate::models::short_path(&self.sessions[idx].cwd)
                } else {
                    new
                };
            }
        }
        self.rename_buf.clear();
    }
}
