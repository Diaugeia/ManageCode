//! Central keymap for Browse mode — the single source of truth for which keys
//! do what. One table (`DEFAULT_BINDINGS`) drives three things that used to
//! drift apart: key dispatch (`input::handle_browse`), the footer hints, and
//! the help overlay. Any key can be remapped from `config.keys`.

use std::collections::HashMap;

use crossterm::event::KeyCode;

use crate::config::KeySpec;

/// Every discrete thing the Browse view can do in response to a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BrowseAction {
    Quit,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Open,
    NewClaude,
    LaunchForm,
    NewShell,
    FocusTerminal,
    Filter,
    Rename,
    Refresh,
    ToggleGroup,
    CollapseInactive,
    ExpandAll,
    CycleView,
    ToggleMute,
    Help,
    Settings,
    CostSummary,
    AiSearch,
    AutoName,
    DeleteJunk,
    DeleteEmpty,
    KillTmux,
    Convert,
    MigrateMemory,
    SetTreeRoot,
}

impl BrowseAction {
    /// Stable snake_case name used as the config key for remapping.
    pub fn name(self) -> &'static str {
        use BrowseAction::*;
        match self {
            Quit => "quit",
            Up => "up",
            Down => "down",
            PageUp => "page_up",
            PageDown => "page_down",
            Top => "top",
            Bottom => "bottom",
            Open => "open",
            NewClaude => "new_claude",
            LaunchForm => "launch_form",
            NewShell => "new_shell",
            FocusTerminal => "focus_terminal",
            Filter => "filter",
            Rename => "rename",
            Refresh => "refresh",
            ToggleGroup => "toggle_group",
            CollapseInactive => "collapse_inactive",
            ExpandAll => "expand_all",
            CycleView => "cycle_view",
            ToggleMute => "toggle_mute",
            Help => "help",
            Settings => "settings",
            CostSummary => "cost_summary",
            AiSearch => "ai_search",
            AutoName => "auto_name",
            DeleteJunk => "delete_junk",
            DeleteEmpty => "delete_empty",
            KillTmux => "kill_tmux",
            Convert => "convert",
            MigrateMemory => "migrate_memory",
            SetTreeRoot => "set_tree_root",
        }
    }

    fn from_name(s: &str) -> Option<BrowseAction> {
        use BrowseAction::*;
        // Walk the known actions so the mapping stays in sync with name().
        [
            Quit,
            Up,
            Down,
            PageUp,
            PageDown,
            Top,
            Bottom,
            Open,
            NewClaude,
            LaunchForm,
            NewShell,
            FocusTerminal,
            Filter,
            Rename,
            Refresh,
            ToggleGroup,
            CollapseInactive,
            ExpandAll,
            CycleView,
            ToggleMute,
            Help,
            Settings,
            CostSummary,
            AiSearch,
            AutoName,
            DeleteJunk,
            DeleteEmpty,
            KillTmux,
            Convert,
            MigrateMemory,
            SetTreeRoot,
        ]
        .into_iter()
        .find(|a| a.name() == s)
    }
}

/// The subset of keys Browse binds (a `KeyCode` we know how to match + display).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Chord {
    Char(char),
    Up,
    Down,
    PageUp,
    PageDown,
    Enter,
    Tab,
}

impl Chord {
    pub fn from_code(code: KeyCode) -> Option<Chord> {
        match code {
            KeyCode::Char(c) => Some(Chord::Char(c)),
            KeyCode::Up => Some(Chord::Up),
            KeyCode::Down => Some(Chord::Down),
            KeyCode::PageUp => Some(Chord::PageUp),
            KeyCode::PageDown => Some(Chord::PageDown),
            KeyCode::Enter => Some(Chord::Enter),
            KeyCode::Tab => Some(Chord::Tab),
            _ => None,
        }
    }

    pub fn glyph(&self) -> String {
        match self {
            Chord::Char(' ') => "space".to_string(),
            Chord::Char(c) => c.to_string(),
            Chord::Up => "↑".to_string(),
            Chord::Down => "↓".to_string(),
            Chord::PageUp => "PgUp".to_string(),
            Chord::PageDown => "PgDn".to_string(),
            Chord::Enter => "⏎".to_string(),
            Chord::Tab => "tab".to_string(),
        }
    }
}

/// One row of the keymap: an action, its default keys, and how it presents in
/// the help overlay and footer.
pub struct Binding {
    pub action: BrowseAction,
    pub keys: &'static [Chord],
    /// Help group header; `None` means it isn't listed in help (its key is
    /// already covered by a sibling row, e.g. Down under Up).
    pub group: Option<&'static str>,
    /// Help key label override (e.g. "g / G" covering Top + Bottom). When None,
    /// the label is generated from `keys`.
    pub help_keys: Option<&'static str>,
    pub help: Option<&'static str>,
    /// `(glyph, label)` if shown in the wide footer.
    pub footer: Option<(&'static str, &'static str)>,
    /// Also show this footer entry in the narrow footer.
    pub footer_narrow: bool,
}

/// Help groups, in display order.
pub const GROUPS: &[&str] = &[
    "navigation",
    "session actions",
    "tmux multi-session",
    "search & AI",
    "maintenance",
];

use BrowseAction as A;
use Chord::*;

pub const DEFAULT_BINDINGS: &[Binding] = &[
    Binding {
        action: A::Up,
        keys: &[Up, Char('k')],
        group: Some("navigation"),
        help_keys: Some("↑ ↓ / k j"),
        help: Some("move selection"),
        footer: Some(("↑↓", "nav")),
        footer_narrow: false,
    },
    Binding {
        action: A::Down,
        keys: &[Down, Char('j')],
        group: None,
        help_keys: None,
        help: None,
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::PageUp,
        keys: &[PageUp],
        group: None,
        help_keys: None,
        help: None,
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::PageDown,
        keys: &[PageDown],
        group: None,
        help_keys: None,
        help: None,
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::Top,
        keys: &[Char('g')],
        group: Some("navigation"),
        help_keys: Some("g / G"),
        help: Some("first / last"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::Bottom,
        keys: &[Char('G')],
        group: None,
        help_keys: None,
        help: None,
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::ToggleGroup,
        keys: &[Char(' '), Tab],
        group: Some("navigation"),
        help_keys: Some("space / tab"),
        help: Some("collapse / expand group"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::CollapseInactive,
        keys: &[Char('o')],
        group: Some("navigation"),
        help_keys: Some("o / O"),
        help: Some("collapse inactive / expand all"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::ExpandAll,
        keys: &[Char('O')],
        group: None,
        help_keys: None,
        help: None,
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::CycleView,
        keys: &[Char('T')],
        group: Some("navigation"),
        help: Some("cycle view: grouped → tree → flat"),
        help_keys: None,
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::SetTreeRoot,
        keys: &[Char('b')],
        group: Some("navigation"),
        help_keys: None,
        help: Some("set tree root directory"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::Open,
        keys: &[Enter],
        group: Some("session actions"),
        help_keys: Some("⏎"),
        help: Some("resume selected session"),
        footer: Some(("⏎", "resume")),
        footer_narrow: true,
    },
    Binding {
        action: A::NewClaude,
        keys: &[Char('n')],
        group: Some("session actions"),
        help_keys: None,
        help: Some("new claude (defaults)"),
        footer: Some(("n", "new claude")),
        footer_narrow: true,
    },
    Binding {
        action: A::LaunchForm,
        keys: &[Char('N')],
        group: Some("session actions"),
        help_keys: None,
        help: Some("new claude (with options)"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::NewShell,
        keys: &[Char('s')],
        group: Some("session actions"),
        help_keys: None,
        help: Some("new shell in cwd (terminal, no claude)"),
        footer: Some(("s", "shell")),
        footer_narrow: true,
    },
    Binding {
        action: A::Rename,
        keys: &[Char('r')],
        group: Some("session actions"),
        help_keys: None,
        help: Some("rename"),
        footer: Some(("r", "rename")),
        footer_narrow: false,
    },
    Binding {
        action: A::Convert,
        keys: &[Char('x')],
        group: Some("session actions"),
        help_keys: None,
        help: Some("convert record to the other tool (Claude↔Codex)"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::MigrateMemory,
        keys: &[Char('X')],
        group: Some("session actions"),
        help_keys: None,
        help: Some("migrate memory (CLAUDE.md/AGENTS.md) to another dir"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::KillTmux,
        keys: &[Char('K')],
        group: Some("tmux multi-session"),
        help_keys: None,
        help: Some("kill the background tmux session"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::Filter,
        keys: &[Char('/')],
        group: Some("search & AI"),
        help_keys: None,
        help: Some("literal filter"),
        footer: Some(("/", "filter")),
        footer_narrow: true,
    },
    Binding {
        action: A::AiSearch,
        keys: &[Char('\\')],
        group: Some("search & AI"),
        help_keys: None,
        help: Some("AI search (Haiku)"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::AutoName,
        keys: &[Char('A')],
        group: Some("search & AI"),
        help_keys: None,
        help: Some("auto-name unnamed sessions"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::DeleteJunk,
        keys: &[Char('D')],
        group: Some("maintenance"),
        help_keys: None,
        help: Some("delete junk sessions"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::DeleteEmpty,
        keys: &[Char('E')],
        group: Some("maintenance"),
        help_keys: None,
        help: Some("delete empty sessions"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::ToggleMute,
        keys: &[Char('M')],
        group: Some("maintenance"),
        help_keys: None,
        help: Some("toggle desktop notifications"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::Refresh,
        keys: &[Char('R')],
        group: Some("maintenance"),
        help_keys: None,
        help: Some("refresh now"),
        footer: Some(("R", "refresh")),
        footer_narrow: false,
    },
    Binding {
        action: A::Settings,
        keys: &[Char(':')],
        group: Some("maintenance"),
        help_keys: None,
        help: Some("settings (terminal prefix, budget)"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::CostSummary,
        keys: &[Char('c')],
        group: Some("maintenance"),
        help_keys: None,
        help: Some("cost summary"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::FocusTerminal,
        keys: &[Char('i'), Char('l')],
        group: Some("maintenance"),
        help_keys: Some("i / l"),
        help: Some("focus embedded terminal"),
        footer: None,
        footer_narrow: false,
    },
    Binding {
        action: A::Help,
        keys: &[Char('?')],
        group: Some("maintenance"),
        help_keys: None,
        help: Some("this help"),
        footer: Some(("?", "help")),
        footer_narrow: true,
    },
    Binding {
        action: A::Quit,
        keys: &[Char('q')],
        group: Some("maintenance"),
        help_keys: Some("q / ctrl-c"),
        help: Some("quit"),
        footer: Some(("q", "quit")),
        footer_narrow: true,
    },
];

/// A help row for the overlay, generated from the active bindings.
pub struct HelpRow {
    pub group: &'static str,
    pub keys: String,
    pub desc: &'static str,
}

/// The resolved keymap: defaults with any `config.keys` char overrides applied.
pub struct Keymap {
    forward: HashMap<Chord, BrowseAction>,
    keys_of: HashMap<BrowseAction, Vec<Chord>>,
}

impl Keymap {
    pub fn from_config(overrides: &HashMap<String, String>) -> Keymap {
        let mut keys_of: HashMap<BrowseAction, Vec<Chord>> = HashMap::new();
        for b in DEFAULT_BINDINGS {
            keys_of.insert(b.action, b.keys.to_vec());
        }
        // Apply char overrides: replace an action's char key(s); keep any
        // non-char defaults (arrows, page keys, enter, tab). Process in a
        // deterministic (action-name) order so two overrides onto the same key
        // resolve identically every run.
        let mut sorted: Vec<(&String, &String)> = overrides.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (name, spec) in sorted {
            if let (Some(action), Ok(ks)) = (BrowseAction::from_name(name), KeySpec::parse(spec)) {
                if let Some(ch) = keyspec_char(&ks) {
                    let entry = keys_of.entry(action).or_default();
                    entry.retain(|c| !matches!(c, Chord::Char(_)));
                    entry.push(Chord::Char(ch));
                }
            }
        }
        // Build the forward lookup in DEFAULT_BINDINGS order; the first action
        // to claim a chord wins. This is deterministic regardless of HashMap
        // iteration order (which is randomized in Rust).
        let mut forward: HashMap<Chord, BrowseAction> = HashMap::new();
        for b in DEFAULT_BINDINGS {
            if let Some(chords) = keys_of.get(&b.action) {
                for c in chords {
                    forward.entry(*c).or_insert(b.action);
                }
            }
        }
        Keymap { forward, keys_of }
    }

    pub fn action_for(&self, code: KeyCode) -> Option<BrowseAction> {
        Chord::from_code(code).and_then(|c| self.forward.get(&c).copied())
    }

    fn label_for(&self, b: &Binding) -> String {
        if let Some(k) = b.help_keys {
            return k.to_string();
        }
        self.keys_of
            .get(&b.action)
            .map(|cs| cs.iter().map(Chord::glyph).collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    }

    /// Help rows in group order, generated from the active bindings.
    pub fn help_rows(&self) -> Vec<HelpRow> {
        let mut rows = Vec::new();
        for b in DEFAULT_BINDINGS {
            if let (Some(group), Some(desc)) = (b.group, b.help) {
                rows.push(HelpRow {
                    group,
                    keys: self.label_for(b),
                    desc,
                });
            }
        }
        rows
    }

    /// Footer hints `(glyph, label)` for the current width.
    pub fn footer_hints(&self, narrow: bool) -> Vec<(String, String)> {
        DEFAULT_BINDINGS
            .iter()
            .filter_map(|b| {
                let (glyph, label) = b.footer?;
                if narrow && !b.footer_narrow {
                    return None;
                }
                Some((glyph.to_string(), label.to_string()))
            })
            .collect()
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::from_config(&HashMap::new())
    }
}

fn keyspec_char(ks: &KeySpec) -> Option<char> {
    match ks.to_crossterm() {
        Some((KeyCode::Char(c), m)) if m.is_empty() => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dispatch() {
        let km = Keymap::default();
        assert_eq!(km.action_for(KeyCode::Char('q')), Some(BrowseAction::Quit));
        assert_eq!(km.action_for(KeyCode::Up), Some(BrowseAction::Up));
        assert_eq!(km.action_for(KeyCode::Char('k')), Some(BrowseAction::Up));
        assert_eq!(km.action_for(KeyCode::Enter), Some(BrowseAction::Open));
        assert_eq!(
            km.action_for(KeyCode::Char('T')),
            Some(BrowseAction::CycleView)
        );
        assert_eq!(km.action_for(KeyCode::Char('z')), None);
    }

    #[test]
    fn config_override_remaps_char_and_frees_old() {
        // 'z' is unused by default, so the remap is unambiguous.
        let mut cfg = HashMap::new();
        cfg.insert("quit".to_string(), "z".to_string());
        let km = Keymap::from_config(&cfg);
        assert_eq!(km.action_for(KeyCode::Char('z')), Some(BrowseAction::Quit));
        assert_eq!(km.action_for(KeyCode::Char('q')), None);
    }

    #[test]
    fn every_help_row_has_a_key_label() {
        let km = Keymap::default();
        let rows = km.help_rows();
        assert!(!rows.is_empty());
        for r in rows {
            assert!(!r.keys.is_empty(), "empty key label for: {}", r.desc);
        }
    }

    #[test]
    fn conflicting_remaps_are_deterministic() {
        // Map both Quit and Filter onto 'x'; the winner must be stable.
        let mut cfg = HashMap::new();
        cfg.insert("quit".to_string(), "x".to_string());
        cfg.insert("filter".to_string(), "x".to_string());
        let a = Keymap::from_config(&cfg).action_for(KeyCode::Char('x'));
        let b = Keymap::from_config(&cfg).action_for(KeyCode::Char('x'));
        assert_eq!(a, b);
        assert!(a.is_some());
    }

    #[test]
    fn action_names_roundtrip() {
        for b in DEFAULT_BINDINGS {
            assert_eq!(BrowseAction::from_name(b.action.name()), Some(b.action));
        }
    }
}
