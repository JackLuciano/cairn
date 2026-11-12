use std::path::Path;

use crate::Result;

/// The ways a file can be ignored, from most specific to broadest.
///
/// # Arguments
/// * `path` - the file, relative to the repository root, with `/` separators
///
/// # Returns
/// `.gitignore` patterns: the file itself, every file with its extension, and its folder.
pub fn suggestions(path: &str) -> Vec<String> {
    let mut list = vec![format!("/{path}")];
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some((stem, ext)) = name.rsplit_once('.')
        && !stem.is_empty()
        && !ext.is_empty()
    {
        list.push(format!("*.{ext}"));
    }
    if let Some((dir, _)) = path.rsplit_once('/') {
        list.push(format!("/{dir}/"));
    }
    list
}

/// Adds a pattern to the repository's `.gitignore`, creating the file when needed.
///
/// # Arguments
/// * `root` - repository root
/// * `pattern` - the line to add
///
/// # Returns
/// `true` when the pattern was added, `false` when it was already there.
pub fn add(root: &Path, pattern: &str) -> Result<bool> {
    let file = root.join(".gitignore");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == pattern) {
        return Ok(false);
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(pattern);
    text.push('\n');
    std::fs::write(&file, text)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_file_extension_and_folder() {
        assert_eq!(suggestions("build/out.log"), vec!["/build/out.log", "*.log", "/build/"]);
        assert_eq!(suggestions("Makefile"), vec!["/Makefile"]);
        assert_eq!(suggestions(".env"), vec!["/.env"]);
    }

    #[test]
    fn adds_each_pattern_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target").unwrap();
        assert!(add(dir.path(), "*.log").unwrap());
        assert!(!add(dir.path(), "*.log").unwrap());
        assert_eq!(std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(), "target\n*.log\n");
    }
}
