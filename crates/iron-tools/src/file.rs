use crate::error::ToolError;
use regex::Regex;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

/// Maximum output size in characters for read_file.
const MAX_OUTPUT_CHARS: usize = 100_000;

// ── Public result types ───────────────────────────────────────────────────────

pub struct ReadResult {
    pub content: String,
    pub total_lines: usize,
    pub truncated: bool,
}

pub struct WriteResult {
    pub success: bool,
    pub lines_written: usize,
}

pub struct PatchResult {
    pub success: bool,
    pub replacements: usize,
    pub diff: String,
}

pub struct SearchMatch {
    pub path: String,
    pub line_number: Option<usize>,
    pub content: Option<String>,
}

// ── read_file ─────────────────────────────────────────────────────────────────

/// Read a file and return its content with 1-indexed line numbers.
///
/// `offset` is the 1-indexed line to start from; `limit` is the maximum number
/// of lines to include.  Block device paths such as `/dev/` and `/proc/self/fd/`
/// are rejected.
pub fn read_file(path: &Path, offset: usize, limit: usize) -> Result<ReadResult, ToolError> {
    // Reject block-device-like paths.
    let path_str = path.to_string_lossy();
    if path_str.starts_with("/dev/") || path_str.starts_with("/proc/self/fd/") {
        return Err(ToolError::ExecutionFailed(format!(
            "reading block/device files is not permitted: {path_str}"
        )));
    }

    let file = fs::File::open(path)
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to open {path_str}: {e}")))?;

    let reader = io::BufReader::new(file);
    let all_lines: Vec<String> = reader
        .lines()
        .collect::<Result<_, _>>()
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to read {path_str}: {e}")))?;

    let total_lines = all_lines.len();

    // offset is 1-indexed; clamp to valid range.
    let start = if offset == 0 {
        0
    } else {
        offset.saturating_sub(1)
    };
    let start = start.min(total_lines);

    let slice = &all_lines[start..];
    let slice = if limit > 0 && slice.len() > limit {
        &slice[..limit]
    } else {
        slice
    };

    let mut output = String::new();
    let mut truncated = false;

    for (i, line) in slice.iter().enumerate() {
        let line_no = start + i + 1;
        let formatted = format!("{line_no}\t{line}\n");

        if output.len() + formatted.len() > MAX_OUTPUT_CHARS {
            truncated = true;
            break;
        }
        output.push_str(&formatted);
    }

    Ok(ReadResult {
        content: output,
        total_lines,
        truncated,
    })
}

// ── write_file ────────────────────────────────────────────────────────────────

/// Write `content` to `path`, creating parent directories as needed.
/// Completely overwrites any existing file.
pub fn write_file(path: &Path, content: &str) -> Result<WriteResult, ToolError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "failed to create parent directories for {}: {e}",
                path.display()
            ))
        })?;
    }

    fs::write(path, content).map_err(|e| {
        ToolError::ExecutionFailed(format!("failed to write {}: {e}", path.display()))
    })?;

    let lines_written = content.lines().count();

    Ok(WriteResult {
        success: true,
        lines_written,
    })
}

// ── patch_file ────────────────────────────────────────────────────────────────

/// Replace occurrences of `old_string` with `new_string` in `path`.
///
/// When `replace_all` is false the function errors if `old_string` appears more
/// than once.  The returned diff has the format `-{old}\n+{new}`.
pub fn patch_file(
    path: &Path,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<PatchResult, ToolError> {
    let original = fs::read_to_string(path).map_err(|e| {
        ToolError::ExecutionFailed(format!("failed to read {}: {e}", path.display()))
    })?;

    let count = original.matches(old_string).count();

    if count == 0 {
        return Err(ToolError::ExecutionFailed(format!(
            "old_string not found in {}",
            path.display()
        )));
    }

    if count > 1 && !replace_all {
        return Err(ToolError::ExecutionFailed(format!(
            "old_string appears {count} times in {}; set replace_all=true to replace all",
            path.display()
        )));
    }

    let patched = if replace_all {
        original.replace(old_string, new_string)
    } else {
        original.replacen(old_string, new_string, 1)
    };

    let replacements = if replace_all { count } else { 1 };

    fs::write(path, &patched).map_err(|e| {
        ToolError::ExecutionFailed(format!("failed to write {}: {e}", path.display()))
    })?;

    let diff = format!("-{old_string}\n+{new_string}");

    Ok(PatchResult {
        success: true,
        replacements,
        diff,
    })
}

// ── search_files ──────────────────────────────────────────────────────────────

/// Search files under `dir`.
///
/// When `is_glob` is true the `pattern` is treated as a filename glob (supports
/// `*` and `?`); results include path only.  When `is_glob` is false the
/// `pattern` is a regex applied to file contents; results include path,
/// line_number, and matching content.
///
/// `file_glob` optionally restricts which files are scanned during content
/// search.  Hidden directories, `node_modules`, and `target` are skipped.
/// At most `limit` results are returned.
pub fn search_files(
    dir: &Path,
    pattern: &str,
    is_glob: bool,
    file_glob: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchMatch>, ToolError> {
    let mut results: Vec<SearchMatch> = Vec::new();

    if is_glob {
        walk_glob(dir, pattern, limit, &mut results);
    } else {
        let re = Regex::new(pattern).map_err(|e| ToolError::InvalidArgs {
            tool: "search_files".to_string(),
            reason: format!("invalid regex '{pattern}': {e}"),
        })?;
        walk_content(dir, &re, file_glob, limit, &mut results)?;
    }

    Ok(results)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_skipped_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "target"
}

/// Recursively walk `dir` collecting files whose names match the glob pattern.
fn walk_glob(dir: &Path, pattern: &str, limit: usize, results: &mut Vec<SearchMatch>) {
    if results.len() >= limit {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if results.len() >= limit {
            break;
        }
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if file_type.is_dir() {
            if !is_skipped_dir(&name_str) {
                walk_glob(&entry.path(), pattern, limit, results);
            }
        } else if file_type.is_file() && glob_match(pattern, &name_str) {
            results.push(SearchMatch {
                path: entry.path().to_string_lossy().into_owned(),
                line_number: None,
                content: None,
            });
        }
    }
}

/// Recursively walk `dir` searching file contents with `re`.
fn walk_content(
    dir: &Path,
    re: &Regex,
    file_glob: Option<&str>,
    limit: usize,
    results: &mut Vec<SearchMatch>,
) -> Result<(), ToolError> {
    if results.len() >= limit {
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        if results.len() >= limit {
            break;
        }
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if file_type.is_dir() {
            if !is_skipped_dir(&name_str) {
                walk_content(&entry.path(), re, file_glob, limit, results)?;
            }
        } else if file_type.is_file() {
            // Apply file_glob filter if provided.
            if let Some(fg) = file_glob
                && !glob_match(fg, &name_str)
            {
                continue;
            }

            search_file_content(&entry.path(), re, limit, results);
        }
    }

    Ok(())
}

fn search_file_content(path: &Path, re: &Regex, limit: usize, results: &mut Vec<SearchMatch>) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return, // skip binary / unreadable files
    };

    for (i, line) in content.lines().enumerate() {
        if results.len() >= limit {
            break;
        }
        if re.is_match(line) {
            results.push(SearchMatch {
                path: path.to_string_lossy().into_owned(),
                line_number: Some(i + 1),
                content: Some(line.to_owned()),
            });
        }
    }
}

/// Minimal glob matching supporting `*` (any sequence) and `?` (any char).
fn glob_match(pattern: &str, name: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_bytes(pat: &[u8], s: &[u8]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (Some(&b'*'), _) => {
            // Try consuming zero or more characters of `s`.
            glob_match_bytes(&pat[1..], s) || (!s.is_empty() && glob_match_bytes(pat, &s[1..]))
        }
        (Some(&b'?'), Some(_)) => glob_match_bytes(&pat[1..], &s[1..]),
        (Some(p), Some(c)) if p == c => glob_match_bytes(&pat[1..], &s[1..]),
        _ => false,
    }
}
