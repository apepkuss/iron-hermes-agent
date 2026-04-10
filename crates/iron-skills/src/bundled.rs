use include_dir::{Dir, DirEntry, include_dir};
use std::path::Path;

static BUNDLED_SKILLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../skills");
static BUNDLED_OPTIONAL_SKILLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../optional-skills");

/// Extract bundled skills to the user's skills directory on first run.
/// Only writes SKILL.md files that do not already exist (preserves user edits).
pub fn extract_bundled_skills(target_dir: &Path) -> anyhow::Result<u32> {
    let mut count = 0;
    count += extract_dir(&BUNDLED_SKILLS, target_dir)?;
    count += extract_dir(&BUNDLED_OPTIONAL_SKILLS, target_dir)?;
    Ok(count)
}

fn extract_dir(source: &Dir, target_dir: &Path) -> anyhow::Result<u32> {
    let mut count = 0;
    for entry in source.entries() {
        if let DirEntry::Dir(d) = entry {
            count += extract_skill_dir(d, target_dir)?;
        }
        // skip top-level files (e.g. DESCRIPTION.md)
    }
    Ok(count)
}

/// Recursively walk an embedded directory, writing SKILL.md files to disk.
///
/// `dir`        — the embedded directory currently being processed
/// `target_dir` — the filesystem path that corresponds to `dir`'s parent
fn extract_skill_dir(dir: &Dir, target_dir: &Path) -> anyhow::Result<u32> {
    let mut count = 0;

    let dir_name = dir
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let target = target_dir.join(dir_name);

    // Check whether any direct file entry is SKILL.md
    for entry in dir.entries() {
        if let DirEntry::File(f) = entry
            && f.path()
                .file_name()
                .map(|n| n == "SKILL.md")
                .unwrap_or(false)
        {
            let target_skill = target.join("SKILL.md");
            if !target_skill.exists() {
                std::fs::create_dir_all(&target)?;
                std::fs::write(&target_skill, f.contents())?;
                count += 1;
            }
        }
    }

    // Recurse into subdirectories
    for entry in dir.entries() {
        if let DirEntry::Dir(subdir) = entry {
            count += extract_skill_dir(subdir, &target)?;
        }
    }

    Ok(count)
}
