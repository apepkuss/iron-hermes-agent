use iron_tools::file::{patch_file, read_file, search_files, write_file};
use std::fs;
use tempfile::TempDir;

// ── read_file ────────────────────────────────────────────────────────────────

#[test]
fn test_read_file_with_line_numbers() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();

    let result = read_file(&path, 1, 100).unwrap();

    assert_eq!(result.total_lines, 3);
    assert!(!result.truncated);
    assert_eq!(result.content, "1\talpha\n2\tbeta\n3\tgamma\n");
}

#[test]
fn test_read_file_with_offset_and_limit() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("hundred.txt");
    let content: String = (1..=100).map(|i| format!("line{i}\n")).collect();
    fs::write(&path, &content).unwrap();

    // offset=10 means start at line 10; limit=5 means read 5 lines (10-14)
    let result = read_file(&path, 10, 5).unwrap();

    assert_eq!(result.total_lines, 100);
    assert!(!result.truncated);
    let expected = "10\tline10\n11\tline11\n12\tline12\n13\tline13\n14\tline14\n";
    assert_eq!(result.content, expected);
}

#[test]
fn test_read_file_blocks_device_files() {
    let result = read_file(std::path::Path::new("/dev/zero"), 1, 10);
    assert!(result.is_err(), "/dev/zero should be rejected");
}

// ── write_file ───────────────────────────────────────────────────────────────

#[test]
fn test_write_file_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sub").join("dir").join("test.txt");

    let result = write_file(&path, "hello\nworld\n").unwrap();

    assert!(result.success);
    assert_eq!(result.lines_written, 2);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld\n");
}

// ── patch_file ───────────────────────────────────────────────────────────────

#[test]
fn test_patch_file_replace() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("patch.txt");
    fs::write(&path, "foo bar baz\n").unwrap();

    let result = patch_file(&path, "bar", "qux", false).unwrap();

    assert!(result.success);
    assert_eq!(result.replacements, 1);
    assert!(result.diff.contains("-bar"));
    assert!(result.diff.contains("+qux"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "foo qux baz\n");
}

#[test]
fn test_patch_file_replace_all() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("patch_all.txt");
    fs::write(&path, "cat cat dog cat\n").unwrap();

    let result = patch_file(&path, "cat", "bird", true).unwrap();

    assert!(result.success);
    assert_eq!(result.replacements, 3);
    assert_eq!(fs::read_to_string(&path).unwrap(), "bird bird dog bird\n");
}

// ── search_files ─────────────────────────────────────────────────────────────

#[test]
fn test_search_files_content() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "hello world\nfoo bar\n").unwrap();
    fs::write(&b, "world peace\nnope\n").unwrap();

    let mut matches = search_files(dir.path(), "world", false, None, 100).unwrap();
    matches.sort_by(|x, y| x.path.cmp(&y.path));

    assert_eq!(matches.len(), 2);
    assert!(matches[0].line_number.is_some());
    assert!(matches[0].content.is_some());
}

#[test]
fn test_search_files_by_glob() {
    let dir = TempDir::new().unwrap();
    let rs_file = dir.path().join("main.rs");
    let txt_file = dir.path().join("readme.txt");
    fs::write(&rs_file, "fn main() {}").unwrap();
    fs::write(&txt_file, "hello").unwrap();

    let matches = search_files(dir.path(), "*.rs", true, None, 100).unwrap();

    assert_eq!(matches.len(), 1);
    assert!(matches[0].path.ends_with("main.rs"));
    assert!(matches[0].line_number.is_none());
    assert!(matches[0].content.is_none());
}
