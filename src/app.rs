use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::ai;
use crate::config::Config;
use crate::models::{SessionInfo, Source};
use crate::notifications::Notifier;
use crate::pty::{TermSession, TerminalSpec};
use crate::scanner;
use crate::tmux;
use crate::tree;
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
    /// A directory node in the tree view. `path` is the full directory path
    /// (the collapse key); `name` is the (possibly path-compressed) label.
    Tree {
        path: String,
        name: String,
        depth: usize,
        total: usize,
        alive: usize,
        collapsed: bool,
    },
    /// A session leaf. `depth` is extra tree-nesting indentation (0 in Grouped
    /// and Flat views; the parent node's depth in Tree view).
    Session { idx: usize, depth: usize }, // idx into App.sessions
}

/// How the sidebar lays out sessions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Flat group headers, one per `cwd`.
    Grouped,
    /// Nested directory tree (path-compressed), sessions as leaves.
    Tree,
    /// No headers — just a flat list of sessions.
    Flat,
}

impl ViewMode {
    fn next(self) -> ViewMode {
        match self {
            ViewMode::Grouped => ViewMode::Tree,
            ViewMode::Tree => ViewMode::Flat,
            ViewMode::Flat => ViewMode::Grouped,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Grouped => "grouped by directory",
            ViewMode::Tree => "directory tree",
            ViewMode::Flat => "flat list",
        }
    }

    /// Parse a config string ("grouped" | "tree" | "flat"); defaults to Grouped.
    pub fn from_label(s: &str) -> ViewMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "tree" => ViewMode::Tree,
            "flat" => ViewMode::Flat,
            _ => ViewMode::Grouped,
        }
    }
}

/// A clickable target in the session list, recorded by the renderer each frame
/// so mouse clicks can be mapped back to a row.
#[derive(Clone)]
pub enum RowHit {
    /// A session row — holds the real index into `App.sessions`.
    Session(usize),
    /// A group header — holds the group's cwd.
    Header(String),
}

/// Top-level UI mode.
pub enum Mode {
    Browse,
    Filter,
    /// Renaming the selected session; `buf` is the in-progress name.
    Rename {
        buf: String,
    },
    Confirm(ConfirmAction),
    Help,
    Launch(LaunchForm),
    /// An embedded terminal pane is active and receiving keystrokes.
    Terminal,
    /// The settings overlay is open.
    Settings(SettingsForm),
    /// The cost-summary overlay is open.
    CostSummary,
    /// Choosing a target directory to migrate the selected session's memory to.
    /// `src` is the source directory; `input` is the target-dir edit buffer.
    MigrateMemory {
        src: String,
        input: String,
    },
    /// Choosing the Tree-view root directory; `input` is the edit buffer.
    TreeRoot {
        input: String,
    },
}

/// Editing state for the settings overlay.
#[derive(Clone)]
pub struct SettingsForm {
    /// Escape-prefix key edit buffer.
    pub input: String,
    /// Daily-budget (USD) edit buffer ("" = off).
    pub budget_input: String,
    /// Which field is focused (0 = prefix, 1 = daily budget).
    pub field: usize,
}

#[derive(Clone)]
pub struct LaunchForm {
    pub cwd: String,
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
    pub fn new(cwd: String, recent_dirs: Vec<String>) -> Self {
        LaunchForm {
            cwd,
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
    if b {
        "[x]".into()
    } else {
        "[ ]".into()
    }
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
    pub mode: Mode,
    pub history_days: i64,
    pub last_scan: Instant,
    pub scanning: bool,
    pub spinner_phase: usize,
    pub message: Option<(String, Instant)>,
    pub custom_names: HashMap<String, String>,
    pub view: ViewMode,
    /// Collapsed directories. In Grouped view these are session `cwd`s; in Tree
    /// view they are tree-node paths (which may be ancestors of a session cwd).
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
    /// Per-frame hit map for the session list: (y, height, target). Written by
    /// the renderer, read by the mouse handler to resolve clicks to rows.
    pub list_hits: RefCell<Vec<(u16, u16, RowHit)>>,
    /// Last left-click (visible-list position, when) for double-click detection.
    pub last_click: Option<(usize, Instant)>,
    /// Tree-view root: only sessions under this dir appear in Tree view. Empty
    /// means "everything" (no scoping). Resolved from config or the launch cwd.
    pub tree_root: String,
    /// Whether the last key was a `z` fold prefix, awaiting `a`/`R`/`M`.
    pub pending_z: bool,
    /// User configuration (escape prefix, etc.).
    pub config: Config,
    /// Browse-mode keymap, resolved from `config.keys` (drives dispatch + UI).
    pub keymap: crate::keymap::Keymap,
    /// Whether we've already alerted that today's spend crossed the budget.
    budget_alerted: bool,
    /// Newer release tag found by the startup check, if any (e.g. "v0.7.0").
    pub update_available: Option<String>,
    last_tmux_refresh: Instant,
    last_live_sweep: Instant,
    dirty_since: Option<Instant>,
    /// The manually-managed mpsc channel ends, grouped to declutter `App`.
    channels: Channels,
}

/// The set of mpsc channel ends `App` manages by hand. Senders are kept so the
/// background threads that produce these events can clone them.
struct Channels {
    update_rx: mpsc::Receiver<String>,
    rx: mpsc::Receiver<Vec<SessionInfo>>,
    tx: mpsc::Sender<Vec<SessionInfo>>,
    ai_rx: mpsc::Receiver<AiEvent>,
    ai_tx: mpsc::Sender<AiEvent>,
    watch_rx: mpsc::Receiver<()>,
}

pub enum AiEvent {
    SearchHit(Option<String>), // matched session id
    NameSuggestion { session_id: String, name: String },
    AutoNameDone,
}

#[derive(Clone)]
pub enum PendingExec {
    Resume {
        id: String,
        cwd: String,
        is_alive: bool,
        source: Source,
    },
    NewClaude {
        cwd: String,
    },
    NewShell {
        cwd: String,
    },
    Custom {
        cwd: String,
        args: Vec<String>,
    },
}

impl App {
    /// `history_days` / `update_check` override the config when `Some` (from CLI
    /// flags); `None` uses the configured value.
    pub fn new(history_days: Option<i64>, update_check: Option<bool>) -> Self {
        let config = crate::config::load();
        let keymap = crate::keymap::Keymap::from_config(&config.keys);
        let history_days = history_days.unwrap_or(config.history_days);
        let view = ViewMode::from_label(&config.default_view);
        // Tree root: configured value, else the directory we were launched from.
        let tree_root = if config.tree_root.is_empty() {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            config.tree_root.clone()
        };
        let notifications = config.notifications;
        let check_updates = update_check.unwrap_or(config.update_check);

        let (tx, rx) = mpsc::channel();
        let (ai_tx, ai_rx) = mpsc::channel();
        let (watch_tx, watch_rx) = mpsc::channel();
        let watcher_active = watcher::spawn(watch_tx);
        // Background check for a newer release; result arrives via update_rx.
        let (update_tx, update_rx) = mpsc::channel();
        if check_updates {
            thread::spawn(move || {
                if let Some(tag) = crate::update::latest_if_newer() {
                    let _ = update_tx.send(tag);
                }
            });
        }
        let mut app = App {
            sessions: Vec::new(),
            selected: 0,
            filter: String::new(),
            mode: Mode::Browse,
            history_days,
            last_scan: Instant::now() - Duration::from_secs(60),
            scanning: false,
            spinner_phase: 0,
            message: None,
            custom_names: scanner::load_custom_names(),
            view,
            collapsed_groups: HashSet::new(),
            ai_running: false,
            auto_naming: false,
            auto_name_progress: (0, 0),
            notifier: Notifier::new(notifications),
            watcher_active,
            tmux_available: tmux::available(),
            tmux_backed: HashSet::new(),
            term: None,
            pending_terminal: None,
            term_area: Cell::new((0, 0, 80, 24)),
            list_hits: RefCell::new(Vec::new()),
            last_click: None,
            tree_root,
            pending_z: false,
            config,
            keymap,
            budget_alerted: false,
            update_available: None,
            last_tmux_refresh: Instant::now() - Duration::from_secs(60),
            last_live_sweep: Instant::now() - Duration::from_secs(60),
            dirty_since: None,
            channels: Channels {
                update_rx,
                rx,
                tx,
                ai_rx,
                ai_tx,
                watch_rx,
            },
        };
        app.kick_scan();
        // Persistent, switch-back-and-forth sessions rely on tmux; nudge the
        // user to install it when it's missing (and we'd want to use it).
        if app.config.prefer_tmux && !app.tmux_available && !tmux::inside_tmux() {
            app.flash(
                "tmux not found — install tmux for persistent sessions you can detach & resume",
            );
        }
        app
    }

    pub fn kick_scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        let tx = self.channels.tx.clone();
        let opts = scanner::ScanOpts {
            history_days: self.history_days,
            scan_claude: self.config.scan_claude,
            scan_codex: self.config.scan_codex,
            max_jsonl_bytes: self.config.refresh.max_jsonl_mb * 1024 * 1024,
        };
        thread::spawn(move || {
            let result = scanner::scan(&opts);
            let _ = tx.send(result);
        });
    }

    pub fn tick(&mut self) {
        self.spinner_phase = (self.spinner_phase + 1) % 8;
        if let Ok(result) = self.channels.rx.try_recv() {
            self.sessions = result;
            self.scanning = false;
            self.last_scan = Instant::now();
            self.notifier.observe(&self.sessions);
            self.clamp_selection();
        }
        // Drain AI events that arrived.
        while let Ok(ev) = self.channels.ai_rx.try_recv() {
            self.handle_ai_event(ev);
        }
        // Pick up the background update-check result (fires at most once).
        if self.update_available.is_none() {
            if let Ok(tag) = self.channels.update_rx.try_recv() {
                self.update_available = Some(tag);
            }
        }
        // Drain file-watcher events — coalesce into a single dirty flag.
        let mut got_event = false;
        while let Ok(()) = self.channels.watch_rx.try_recv() {
            got_event = true;
        }
        if got_event {
            self.dirty_since = Some(Instant::now());
        }
        // Debounced event-driven scan after the last event.
        if let Some(t) = self.dirty_since {
            if t.elapsed() >= Duration::from_millis(self.config.refresh.debounce_ms)
                && !self.scanning
            {
                self.kick_scan();
                self.dirty_since = None;
            }
        }
        // Lightweight PID/status sweep — picks up busy↔idle quickly without
        // touching JSONL files.
        // `.max(..)` floors guard against a 0 in the config busy-looping the scan.
        if self.last_live_sweep.elapsed()
            >= Duration::from_millis(self.config.refresh.live_ms.max(250))
        {
            scanner::refresh_live_status(&mut self.sessions);
            self.notifier.observe(&self.sessions);
            self.last_live_sweep = Instant::now();
        }
        // Refresh the set of background tmux sessions.
        if self.tmux_available
            && self.last_tmux_refresh.elapsed()
                >= Duration::from_millis(self.config.refresh.tmux_ms.max(250))
        {
            self.refresh_tmux_backed();
            self.last_tmux_refresh = Instant::now();
        }
        // Fallback full scan. Faster when watcher couldn't attach.
        let full_secs = self.config.refresh.full_secs.max(2);
        let fallback = if self.watcher_active {
            Duration::from_secs(full_secs)
        } else {
            Duration::from_secs(full_secs.min(5))
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
                    Some(id) => match self.sessions.iter().position(|s| s.id == id) {
                        Some(real) if self.select_by_real_index(real).is_some() => {
                            self.flash("AI: found a match");
                        }
                        Some(real) => {
                            // Hidden by filter or a collapsed group; expose it.
                            let cwd = self.sessions[real].cwd.clone();
                            self.reveal_path(&cwd);
                            self.filter.clear();
                            if self.select_by_real_index(real).is_some() {
                                self.flash("AI: cleared filter to reveal match");
                            } else {
                                self.flash("AI: match not in current list");
                            }
                        }
                        None => self.flash("AI: match not in current list"),
                    },
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
        let tx = self.channels.ai_tx.clone();
        let sessions = self.sessions.clone();
        let model = self.config.ai_model.clone();
        let timeout = self.config.ai_timeout_secs;
        std::thread::spawn(move || {
            let id = ai::search(&query, &sessions, &model, timeout);
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
        let tx = self.channels.ai_tx.clone();
        let projects_dir = scanner::claude_dir().join("projects");
        let model = self.config.ai_model.clone();
        let timeout = self.config.ai_timeout_secs;
        std::thread::spawn(move || {
            for (id, _) in cands {
                let snippet = match ai::sample_session_text(&projects_dir, &id) {
                    Some(s) => s,
                    None => continue,
                };
                if let Some(name) = ai::suggest_name(&snippet, &model, timeout) {
                    let _ = tx.send(AiEvent::NameSuggestion {
                        session_id: id,
                        name,
                    });
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

    /// Headers / tree nodes + sessions woven together in display order.
    pub fn visible_rows(&self) -> Vec<Row> {
        let filtered = self.filtered_indices();
        match self.view {
            ViewMode::Flat => filtered
                .into_iter()
                .map(|idx| Row::Session { idx, depth: 0 })
                .collect(),
            ViewMode::Grouped => self.grouped_rows(&filtered),
            ViewMode::Tree => self.tree_rows(&filtered),
        }
    }

    fn grouped_rows(&self, filtered: &[usize]) -> Vec<Row> {
        // Pre-count sessions per cwd in a single pass so each header lookup is
        // O(1) — same totals as before, but O(n) instead of O(groups·n).
        let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
        for &i in filtered {
            let s = &self.sessions[i];
            let e = counts.entry(s.cwd.as_str()).or_insert((0, 0));
            e.0 += 1;
            if s.is_alive {
                e.1 += 1;
            }
        }

        let mut out: Vec<Row> = Vec::new();
        let mut last_cwd: Option<&str> = None;
        for &idx in filtered {
            let s = &self.sessions[idx];
            if last_cwd != Some(s.cwd.as_str()) {
                last_cwd = Some(s.cwd.as_str());
                let (total, alive) = counts.get(s.cwd.as_str()).copied().unwrap_or((0, 0));
                out.push(Row::Header {
                    cwd: s.cwd.clone(),
                    total,
                    alive,
                    collapsed: self.collapsed_groups.contains(&s.cwd),
                });
            }
            if !self.collapsed_groups.contains(&s.cwd) {
                out.push(Row::Session { idx, depth: 0 });
            }
        }
        out
    }

    fn tree_rows(&self, filtered: &[usize]) -> Vec<Row> {
        tree::tree_rows(
            &self.sessions,
            filtered,
            &self.collapsed_groups,
            matches!(self.view, ViewMode::Tree),
            &self.tree_root,
        )
    }

    /// The compressed paths of the immediate child nodes of the tree node
    /// rendered at `parent_path`. Used to collapse children one level on expand
    /// (file-explorer behavior). Empty if the node has no child directories.
    fn immediate_child_paths(&self, parent_path: &str) -> Vec<String> {
        let filtered = self.filtered_indices();
        tree::immediate_child_paths(
            &self.sessions,
            &filtered,
            matches!(self.view, ViewMode::Tree),
            &self.tree_root,
            parent_path,
        )
    }

    /// The session under the cursor, or `None` if the cursor is on a directory
    /// header / tree node (which are also selectable rows now).
    pub fn selected_session(&self) -> Option<&SessionInfo> {
        let rows = self.visible_rows();
        if let Some(Row::Session { idx, .. }) = rows.get(self.selected) {
            self.sessions.get(*idx)
        } else {
            None
        }
    }

    /// Cycle the sidebar layout: grouped → tree → flat.
    pub fn cycle_view(&mut self) {
        self.view = self.view.next();
        self.clamp_selection();
        self.flash(self.view.label());
    }

    /// Toggle the collapse state of the row under the cursor. On a header / tree
    /// node that's the node itself; on a session it's the session's directory.
    pub fn toggle_group_of_selection(&mut self) {
        let key = match self.visible_rows().get(self.selected) {
            Some(Row::Header { cwd, .. }) => cwd.clone(),
            Some(Row::Tree { path, .. }) => path.clone(),
            Some(Row::Session { idx, .. }) => self.sessions[*idx].cwd.clone(),
            None => return,
        };
        self.toggle_collapse(&key);
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

    /// Collapse every group (Grouped) or every directory node (Tree). No-op in
    /// Flat view. Bound to `zM`.
    pub fn collapse_all(&mut self) {
        match self.view {
            ViewMode::Grouped => {
                let cwds: Vec<String> = self.sessions.iter().map(|s| s.cwd.clone()).collect();
                self.collapsed_groups.extend(cwds);
            }
            ViewMode::Tree => {
                // Collapse every ancestor path so the tree folds to its roots.
                let cwds: Vec<String> = self.sessions.iter().map(|s| s.cwd.clone()).collect();
                for cwd in cwds {
                    let mut path = String::new();
                    for c in tree::path_segments(&cwd) {
                        path.push('/');
                        path.push_str(c);
                        self.collapsed_groups.insert(path.clone());
                    }
                }
            }
            ViewMode::Flat => {}
        }
        self.clamp_selection();
    }

    /// Open the Tree-root picker, seeded with the current root (or launch cwd).
    pub fn open_tree_root(&mut self) {
        let input = if self.tree_root.is_empty() {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            self.tree_root.clone()
        };
        self.mode = Mode::TreeRoot { input };
    }

    /// Apply the Tree-root picker: scope the tree, persist, and switch to Tree
    /// view. An empty input clears the root (show everything).
    pub fn apply_tree_root(&mut self, input: &str) {
        let dir = input.trim().to_string();
        self.tree_root = dir.clone();
        self.config.tree_root = dir;
        let _ = crate::config::save(&self.config);
        self.view = ViewMode::Tree;
        self.clamp_selection();
        if self.tree_root.is_empty() {
            self.flash("tree root cleared (showing all)");
        } else {
            self.flash(format!(
                "tree root → {}",
                crate::models::short_path(&self.tree_root)
            ));
        }
        self.mode = Mode::Browse;
    }

    /// Candidate root dirs for the picker: launch cwd, home, then session dirs.
    pub fn root_candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Ok(c) = std::env::current_dir() {
            out.push(c.to_string_lossy().to_string());
        }
        if let Some(h) = dirs::home_dir() {
            let h = h.to_string_lossy().to_string();
            if !out.contains(&h) {
                out.push(h);
            }
        }
        for d in self.recent_dirs() {
            if !out.contains(&d) {
                out.push(d);
            }
        }
        out
    }

    pub fn clamp_selection(&mut self) {
        let len = self.visible_rows().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        if self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        // Selection indexes every visible row (headers, tree nodes, sessions) so
        // a collapsed directory can always be moved onto and reopened.
        let len = self.visible_rows().len();
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
                    let (ids, n) =
                        scanner::delete_sessions(&self.sessions, scanner::is_junk_session);
                    self.sessions.retain(|s| !ids.contains(&s.id));
                    self.clamp_selection();
                    self.flash(format!("deleted {} junk session(s)", n));
                }
                ConfirmAction::DeleteEmpty => {
                    let (ids, n) =
                        scanner::delete_sessions(&self.sessions, scanner::is_empty_session);
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

    /// Select the session with this real index (clicked in the list), if it is
    /// currently visible. Returns the visible-row position chosen.
    pub fn select_by_real_index(&mut self, real_idx: usize) -> Option<usize> {
        let pos = self
            .visible_rows()
            .iter()
            .position(|r| matches!(r, Row::Session { idx, .. } if *idx == real_idx))?;
        self.selected = pos;
        Some(pos)
    }

    /// Toggle a directory's collapsed state by its key (cwd in Grouped, node
    /// path in Tree). In Tree view, expanding reveals only the immediate
    /// children — which stay collapsed — so opening a folder drills one level
    /// at a time instead of fanning the whole subtree open.
    pub fn toggle_collapse(&mut self, key: &str) {
        if self.collapsed_groups.contains(key) {
            self.collapsed_groups.remove(key);
            if matches!(self.view, ViewMode::Tree) {
                for child in self.immediate_child_paths(key) {
                    self.collapsed_groups.insert(child);
                }
            }
        } else {
            self.collapsed_groups.insert(key.to_string());
        }
        self.clamp_selection();
    }

    /// Un-collapse everything hiding `cwd` (the directory itself or any ancestor
    /// node) so the session there becomes visible.
    pub fn reveal_path(&mut self, cwd: &str) {
        self.collapsed_groups
            .retain(|p| !tree::is_ancestor_or_eq(p, cwd));
    }

    /// Move the cursor onto the row for `key` (header / tree node) and toggle it
    /// — used when a header is clicked so the keyboard cursor follows the mouse.
    pub fn select_and_toggle(&mut self, key: &str) {
        if let Some(pos) = self.visible_rows().iter().position(|r| match r {
            Row::Header { cwd, .. } => cwd == key,
            Row::Tree { path, .. } => path == key,
            _ => false,
        }) {
            self.selected = pos;
        }
        self.toggle_collapse(key);
    }

    /// Distinct session cwds, most-recently-active first (sessions are already
    /// sorted by recency), for quick selection in the launch form.
    /// Open the memory-migration overlay for the selected session's directory.
    pub fn open_migrate(&mut self) {
        let Some(s) = self.selected_session() else {
            self.flash("no selection");
            return;
        };
        let src = s.cwd.clone();
        if !crate::memory::has_memory(&src) {
            self.flash("no CLAUDE.md / AGENTS.md in this session's directory");
            return;
        }
        let input = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_default();
        self.mode = Mode::MigrateMemory { src, input };
    }

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
        let input = self.config.escape_prefix.label();
        let budget_input = self
            .config
            .daily_budget_usd
            .map(|v| format!("{v}"))
            .unwrap_or_default();
        self.mode = Mode::Settings(SettingsForm {
            input,
            budget_input,
            field: 0,
        });
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

    pub fn rename_selected(&mut self, buf: &str) {
        if let Some(s) = self.selected_session() {
            let id = s.id.clone();
            let new = buf.trim().to_string();
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
    }
}
