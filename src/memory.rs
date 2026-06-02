//! Migrate a project's memory / instructions file (Claude Code's `CLAUDE.md`,
//! OpenAI Codex's `AGENTS.md`) from one directory to another — across
//! directories and across tools — so renaming or relocating a project doesn't
//! lose its memory.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};

/// The instruction-file names both tools recognize.
pub const MEMORY_FILES: &[&str] = &["CLAUDE.md", "AGENTS.md"];

/// Does `dir` have a non-empty CLAUDE.md or AGENTS.md?
pub fn has_memory(dir: &str) -> bool {
    MEMORY_FILES.iter().any(|name| {
        fs::read_to_string(Path::new(dir).join(name))
            .map(|c| !c.trim().is_empty())
            .unwrap_or(false)
    })
}

/// Read the source memory: the first of CLAUDE.md / AGENTS.md that exists and
/// is non-empty in `src_dir`.
fn read_source(src_dir: &str) -> Result<String> {
    for name in MEMORY_FILES {
        if let Ok(c) = fs::read_to_string(Path::new(src_dir).join(name)) {
            if !c.trim().is_empty() {
                return Ok(c);
            }
        }
    }
    Err(anyhow!("no CLAUDE.md or AGENTS.md in {src_dir}"))
}

/// Copy `src_dir`'s memory into `dst_dir`, written as BOTH `CLAUDE.md` and
/// `AGENTS.md` so either tool picks it up there (cross-tool by design).
/// Existing destination files are appended to with a marker, never clobbered.
/// Returns how many files were created or updated.
pub fn migrate_memory(src_dir: &str, dst_dir: &str) -> Result<usize> {
    let src_dir = src_dir.trim_end_matches('/');
    let dst_dir = dst_dir.trim_end_matches('/');
    if src_dir == dst_dir {
        return Err(anyhow!("source and target directory are the same"));
    }
    let dst = Path::new(dst_dir);
    if !dst.is_dir() {
        return Err(anyhow!("target is not a directory: {dst_dir}"));
    }
    let content = read_source(src_dir)?;

    let mut touched = 0;
    for name in MEMORY_FILES {
        if write_or_append(&dst.join(name), &content, src_dir)? {
            touched += 1;
        }
    }
    Ok(touched)
}

/// Write `content` to `target`, or append it (with a provenance marker) if the
/// file already exists. Returns false if the content was already present.
fn write_or_append(target: &Path, content: &str, src_dir: &str) -> Result<bool> {
    if target.exists() {
        let existing = fs::read_to_string(target).unwrap_or_default();
        if existing.contains(content.trim()) {
            return Ok(false);
        }
        let merged = format!("{existing}\n\n<!-- migrated from {src_dir} -->\n{content}");
        fs::write(target, merged)?;
    } else {
        fs::write(target, content)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_copies_to_both_tool_names() {
        let base = std::env::temp_dir().join(format!("mc-mem-{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("CLAUDE.md"), "# project memory").unwrap();

        let n = migrate_memory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            fs::read_to_string(dst.join("AGENTS.md")).unwrap(),
            "# project memory"
        );
        assert!(dst.join("CLAUDE.md").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn same_directory_is_rejected() {
        assert!(migrate_memory("/x/y", "/x/y").is_err());
    }
}
