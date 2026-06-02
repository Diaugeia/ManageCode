//! Self-update: a lightweight startup check that surfaces when a newer release
//! exists on GitHub, plus `managecode --update`, which re-runs the published
//! install script. No new crate dependencies — the version check shells out to
//! `curl` (already required to install ManageCode) and parses with serde_json;
//! the updater reuses the tested install.sh path.

use std::process::Command;

const REPO: &str = "Diaugeia/ManageCode";

/// The version this binary was built as (no leading `v`).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Has the user opted out of the startup update check via env var?
pub fn check_disabled() -> bool {
    std::env::var("MANAGECODE_NO_UPDATE_CHECK")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(false)
}

/// Query GitHub for the latest release tag. Returns the tag (e.g. "v0.7.0")
/// only when it is strictly newer than the running version; `None` otherwise
/// (up to date, offline, curl missing, or any error).
pub fn latest_if_newer() -> Option<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github+json",
            // GitHub requires a User-Agent on API requests.
            "-A",
            "managecode-update-check",
            &url,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = json.get("tag_name")?.as_str()?.to_string();
    if is_newer(current_version(), &tag) {
        Some(tag)
    } else {
        None
    }
}

/// Is `tag` (e.g. "v0.7.0") a newer semver than `current` (e.g. "0.6.0")?
fn is_newer(current: &str, tag: &str) -> bool {
    match (parse_semver(current), parse_semver(tag)) {
        (Some(c), Some(t)) => t > c,
        _ => false,
    }
}

/// Parse "v1.2.3" / "1.2.3" into a comparable (major, minor, patch) tuple.
fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut it = s.split('.').map(|p| {
        // Drop any pre-release/build suffix on a component.
        let num: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
        num.parse::<u32>().ok()
    });
    let major = it.next()??;
    let minor = it.next().flatten().unwrap_or(0);
    let patch = it.next().flatten().unwrap_or(0);
    Some((major, minor, patch))
}

/// Run the published install script to replace this binary with the latest
/// release. Inherits stdio so the user sees the installer's own output. Always
/// proceeds (the user asked explicitly); when a newer tag is known it is shown,
/// otherwise we reinstall the latest without assuming we're up to date — a
/// failed version check (offline, rate-limited) must not silently skip it.
pub fn run_update() -> std::io::Result<std::process::ExitStatus> {
    match latest_if_newer() {
        Some(tag) => println!("Updating ManageCode v{} → {tag} …", current_version()),
        None => println!(
            "Reinstalling the latest ManageCode (current v{}) …",
            current_version()
        ),
    }
    let script =
        format!("curl -fsSL https://raw.githubusercontent.com/{REPO}/main/install.sh | bash");
    Command::new("bash").arg("-c").arg(&script).status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_compare() {
        assert!(is_newer("0.6.0", "v0.7.0"));
        assert!(is_newer("0.6.0", "0.6.1"));
        assert!(is_newer("0.6.0", "v1.0.0"));
        assert!(!is_newer("0.6.0", "v0.6.0"));
        assert!(!is_newer("0.6.0", "v0.5.9"));
        assert!(!is_newer("0.6.0", "garbage"));
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.6"), Some((0, 6, 0)));
    }
}
