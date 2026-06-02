//! User configuration, persisted as JSON at `~/.managecode/config.json`.
//! Currently holds the embedded-terminal escape prefix key, which returns
//! focus from the terminal pane back to the sidebar.

use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Key that, while the terminal pane is focused, returns focus to the
    /// sidebar (does not kill the session).
    pub escape_prefix: KeySpec,
    /// Daily spend ceiling in USD; alert when today's cost reaches it. None = off.
    #[serde(default)]
    pub daily_budget_usd: Option<f64>,
    /// Run launches inside a detached tmux session (when tmux is available) so
    /// they persist across detach/quit and you can switch back and forth all
    /// day. Set false to always run directly in the embedded PTY.
    #[serde(default = "default_true")]
    pub prefer_tmux: bool,
    /// On quit, kill all `mc-*` tmux sessions this tool created. Keeps the tmux
    /// server tidy; set false to leave them running in the background.
    #[serde(default = "default_true")]
    pub cleanup_tmux_on_exit: bool,
    /// Per-action key overrides for Browse mode, e.g. {"quit": "x"}. Action
    /// names match `keymap::BrowseAction::name()`. Empty = use the defaults.
    #[serde(default)]
    pub keys: std::collections::HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            escape_prefix: KeySpec::ctrl_a(),
            daily_budget_usd: None,
            prefer_tmux: true,
            cleanup_tmux_on_exit: true,
            keys: std::collections::HashMap::new(),
        }
    }
}

/// A human-authored key combination, e.g. `Ctrl-a`, `F12`, `Ctrl-Space`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".managecode").join("config.json"))
}

/// Load the config, falling back to defaults on any error.
pub fn load() -> Config {
    let Some(p) = config_path() else {
        return Config::default();
    };
    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Persist the config to `~/.managecode/config.json`.
pub fn save(cfg: &Config) -> Result<()> {
    let Some(p) = config_path() else {
        return Ok(());
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(cfg)?)?;
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

        assert!(KeySpec::parse("f12").unwrap().matches(KeyCode::F(12), KeyModifiers::NONE));
        assert!(KeySpec::parse("ctrl-space").is_ok());
        assert!(KeySpec::parse("bogus-x").is_err());
    }

    #[test]
    fn reserved_keys() {
        assert!(KeySpec::parse("ctrl-c").unwrap().is_reserved());
        assert!(KeySpec::parse("ctrl-d").unwrap().is_reserved());
        assert!(!KeySpec::parse("ctrl-a").unwrap().is_reserved());
    }
}
