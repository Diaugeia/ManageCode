//! User configuration, persisted as TOML at `~/.managecode/config.toml`.
//! A legacy `~/.managecode/config.json` is migrated automatically on first load.
//! See `docs/config.md` for the full reference.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    // --- General ---
    /// Initial sidebar layout: "grouped" | "tree" | "flat".
    pub default_view: String,
    /// Show the per-session cost (right-aligned) in the list. Off by default.
    pub show_cost: bool,
    /// Tree-view root directory; only sessions under it are shown. Empty = the
    /// directory ManageCode was launched from. Set at runtime via the root picker.
    pub tree_root: String,
    /// How far back to scan, in days. The `--days` flag overrides this.
    pub history_days: i64,
    /// Daily spend ceiling in USD; alert when today's cost reaches it. None = off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily_budget_usd: Option<f64>,
    /// Desktop notifications when a busy session goes idle.
    pub notifications: bool,
    /// Check GitHub for a newer release on startup.
    pub update_check: bool,

    // --- Sources ---
    /// Scan `~/.claude` for Claude Code sessions.
    pub scan_claude: bool,
    /// Scan `~/.codex` for OpenAI Codex sessions.
    pub scan_codex: bool,
    /// Override the `claude` binary path ("" = `$CLAUDE_BIN`, then `$PATH`).
    pub claude_bin: String,
    /// Override the `codex` binary path ("" = `$CODEX_BIN`, then `$PATH`).
    pub codex_bin: String,

    // --- Terminal / tmux ---
    /// Run launches inside a detached tmux session (when available) so they
    /// persist across detach/quit. Set false to always use the embedded PTY.
    pub prefer_tmux: bool,
    /// On quit, kill all `mc-*` tmux sessions this tool created.
    pub cleanup_tmux_on_exit: bool,

    // --- AI (search + auto-name) ---
    /// Model passed to `claude --print` for AI search and auto-naming.
    pub ai_model: String,
    /// Timeout for an AI call, in seconds.
    pub ai_timeout_secs: u64,

    // --- Tables (must stay after the scalar fields above for valid TOML) ---
    /// Key that returns focus from the terminal pane to the sidebar.
    pub escape_prefix: KeySpec,
    /// Background refresh cadences.
    pub refresh: RefreshConfig,
    /// Per-action key overrides for Browse mode, e.g. `quit = "x"`. Action
    /// names match `keymap::BrowseAction::name()`.
    pub keys: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_view: "grouped".to_string(),
            show_cost: false,
            tree_root: String::new(),
            history_days: 30,
            daily_budget_usd: None,
            notifications: true,
            update_check: true,
            scan_claude: true,
            scan_codex: true,
            claude_bin: String::new(),
            codex_bin: String::new(),
            prefer_tmux: true,
            cleanup_tmux_on_exit: true,
            ai_model: "haiku".to_string(),
            ai_timeout_secs: 45,
            escape_prefix: KeySpec::ctrl_a(),
            refresh: RefreshConfig::default(),
            keys: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct RefreshConfig {
    /// PID / status sweep interval (ms).
    pub live_ms: u64,
    /// tmux backed-set reconcile interval (ms).
    pub tmux_ms: u64,
    /// Fallback full-scan interval when the file watcher is active (seconds).
    pub full_secs: u64,
    /// Debounce after a file event before scanning (ms).
    pub debounce_ms: u64,
    /// Skip transcript files larger than this (MB).
    pub max_jsonl_mb: u64,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        RefreshConfig {
            live_ms: 1500,
            tmux_ms: 2000,
            full_secs: 30,
            debounce_ms: 180,
            max_jsonl_mb: 100,
        }
    }
}

/// A human-authored key combination, e.g. `Ctrl-a`, `F12`, `Ctrl-Space`.
/// Serializes as a plain string (`"ctrl-a"`) so config files read cleanly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySpec {
    /// Modifiers in canonical lowercase: any of "ctrl", "alt", "shift".
    pub mods: Vec<String>,
    /// The base key in canonical lowercase: a single char, or "space", "esc",
    /// "tab", "enter", or "f1".."f12".
    pub code: String,
}

impl KeySpec {
    pub fn ctrl_a() -> Self {
        KeySpec {
            mods: vec!["ctrl".into()],
            code: "a".into(),
        }
    }

    /// Parse a spec like "ctrl-a", "Ctrl+Space", "f12". Validates that it maps
    /// to a real key.
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        let lower = s.trim().to_lowercase();
        let parts: Vec<&str> = lower
            .split(['-', '+'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let Some((code_tok, mod_toks)) = parts.split_last() else {
            return Err("empty key".into());
        };
        let mut mods = Vec::new();
        for m in mod_toks {
            let canon = match *m {
                "ctrl" | "control" | "c" => "ctrl",
                "alt" | "option" | "meta" | "a" => "alt",
                "shift" => "shift",
                other => return Err(format!("unknown modifier: {other}")),
            };
            if !mods.iter().any(|x: &String| x == canon) {
                mods.push(canon.to_string());
            }
        }
        let ks = KeySpec {
            mods,
            code: code_tok.to_string(),
        };
        if ks.to_crossterm().is_none() {
            return Err(format!("unknown key: {code_tok}"));
        }
        Ok(ks)
    }

    /// Resolve to a concrete crossterm key, or `None` if the spec is invalid.
    pub fn to_crossterm(&self) -> Option<(KeyCode, KeyModifiers)> {
        let mut mods = KeyModifiers::NONE;
        for m in &self.mods {
            match m.as_str() {
                "ctrl" => mods |= KeyModifiers::CONTROL,
                "alt" => mods |= KeyModifiers::ALT,
                "shift" => mods |= KeyModifiers::SHIFT,
                _ => return None,
            }
        }
        let code = match self.code.as_str() {
            "space" => KeyCode::Char(' '),
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "enter" | "return" => KeyCode::Enter,
            s if s.chars().count() == 1 => KeyCode::Char(s.chars().next().unwrap()),
            s if s.starts_with('f') => {
                let n: u8 = s[1..].parse().ok()?;
                if (1..=12).contains(&n) {
                    KeyCode::F(n)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some((code, mods))
    }

    /// Does an incoming key event match this spec? Compares Ctrl/Alt and the
    /// base key case-insensitively (Shift is ignored to dodge case quirks).
    pub fn matches(&self, code: KeyCode, mods: KeyModifiers) -> bool {
        let Some((want_code, want_mods)) = self.to_crossterm() else {
            return false;
        };
        let relevant = KeyModifiers::CONTROL | KeyModifiers::ALT;
        if (mods & relevant) != (want_mods & relevant) {
            return false;
        }
        match (code, want_code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        }
    }

    /// Is this a key we refuse to bind as the prefix (would shadow a vital key)?
    pub fn is_reserved(&self) -> bool {
        matches!(
            self.to_crossterm(),
            Some((KeyCode::Char(c), m))
                if m.contains(KeyModifiers::CONTROL) && (c == 'c' || c == 'd')
        )
    }

    /// Human-readable, also re-parseable, e.g. "Ctrl-A".
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = self
            .mods
            .iter()
            .map(|m| match m.as_str() {
                "ctrl" => "Ctrl".to_string(),
                "alt" => "Alt".to_string(),
                "shift" => "Shift".to_string(),
                o => o.to_string(),
            })
            .collect();
        parts.push(if self.code.chars().count() == 1 {
            self.code.to_uppercase()
        } else {
            self.code.clone()
        });
        parts.join("-")
    }
}

impl Serialize for KeySpec {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.label())
    }
}

impl<'de> Deserialize<'de> for KeySpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        // Accept either "ctrl-a" or the legacy { mods, code } object so old
        // config.json files migrate cleanly.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Str(String),
            Map { mods: Vec<String>, code: String },
        }
        match Raw::deserialize(d)? {
            Raw::Str(s) => KeySpec::parse(&s).map_err(de::Error::custom),
            Raw::Map { mods, code } => Ok(KeySpec { mods, code }),
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".managecode"))
}

fn toml_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

fn json_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

/// Load config from `config.toml`, migrating a legacy `config.json` if present.
/// Falls back to defaults on any error.
pub fn load() -> Config {
    if let Some(p) = toml_path() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            return toml::from_str(&s).unwrap_or_default();
        }
    }
    // One-time migration from the old JSON config.
    if let Some(jp) = json_path() {
        if let Ok(s) = std::fs::read_to_string(&jp) {
            if let Ok(cfg) = serde_json::from_str::<Config>(&s) {
                let _ = save(&cfg);
                return cfg;
            }
        }
    }
    Config::default()
}

/// Persist the config to `~/.managecode/config.toml`.
pub fn save(cfg: &Config) -> Result<()> {
    let Some(p) = toml_path() else {
        return Ok(());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, toml::to_string_pretty(cfg)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches() {
        let ks = KeySpec::parse("ctrl-a").unwrap();
        assert!(ks.matches(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(ks.matches(KeyCode::Char('A'), KeyModifiers::CONTROL));
        assert!(!ks.matches(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(ks.label(), "Ctrl-A");

        assert!(KeySpec::parse("f12")
            .unwrap()
            .matches(KeyCode::F(12), KeyModifiers::NONE));
        assert!(KeySpec::parse("ctrl-space").is_ok());
        assert!(KeySpec::parse("bogus-x").is_err());
    }

    #[test]
    fn reserved_keys() {
        assert!(KeySpec::parse("ctrl-c").unwrap().is_reserved());
        assert!(KeySpec::parse("ctrl-d").unwrap().is_reserved());
        assert!(!KeySpec::parse("ctrl-a").unwrap().is_reserved());
    }

    #[test]
    fn toml_roundtrips_with_string_keyspec() {
        let cfg = Config::default();
        let s = toml::to_string_pretty(&cfg).unwrap();
        // escape_prefix is a plain string, and tables come last (valid TOML).
        assert!(s.contains("escape_prefix = \"Ctrl-A\""), "{s}");
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.escape_prefix, cfg.escape_prefix);
        assert_eq!(back.history_days, 30);
        assert_eq!(back.refresh.max_jsonl_mb, 100);
        assert!(back.prefer_tmux);
    }

    #[test]
    fn migrates_legacy_json_keyspec() {
        // Old config.json stored escape_prefix as a { mods, code } object.
        let json = r#"{"escape_prefix":{"mods":["ctrl"],"code":"a"},"daily_budget_usd":12.5}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.escape_prefix, KeySpec::ctrl_a());
        assert_eq!(cfg.daily_budget_usd, Some(12.5));
        // Unspecified fields fall back to defaults.
        assert!(cfg.scan_codex);
    }
}
