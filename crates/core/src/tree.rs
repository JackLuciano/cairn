use crate::git::Git;
use crate::Result;

/// A file or folder in a commit's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

/// What a file looked like at a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileContent {
    Text(String),
    Binary(u64),
}

/// The largest file shown as text; bigger files are offered for saving only.
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

fn parse_ls_tree(out: &[u8]) -> Vec<TreeEntry> {
    let mut entries: Vec<TreeEntry> = out
        .split(|&b| b == 0)
        .filter(|r| !r.is_empty())
        .filter_map(|record| {
            let record = String::from_utf8_lossy(record);
            let (meta, path) = record.split_once('\t')?;
            let mut fields = meta.split_whitespace();
            let (_, kind, _, size) = (fields.next()?, fields.next()?, fields.next()?, fields.next());
            let is_dir = kind == "tree";
            if kind == "commit" {
                return None;
            }
            let name = path.rsplit('/').next().unwrap_or(path).to_string();
            Some(TreeEntry { path: path.to_string(), name, is_dir, size: size.and_then(|s| s.parse().ok()) })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    entries
}

/// Lists one folder of the tree at a commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `rev` - branch, tag or commit
/// * `dir` - the folder, empty for the top
///
/// # Returns
/// Folders first, then files, both by name.
pub fn list_dir(git: &Git, rev: &str, dir: &str) -> Result<Vec<TreeEntry>> {
    let spec = if dir.is_empty() { rev.to_string() } else { format!("{rev}:{dir}") };
    let out = git.run_bytes(["ls-tree", "-z", "--long", &spec])?;
    let mut entries = parse_ls_tree(&out);
    if !dir.is_empty() {
        for e in &mut entries {
            e.path = format!("{dir}/{}", e.path);
        }
    }
    Ok(entries)
}

/// Reads a file as it was at a commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `rev` - branch, tag or commit
/// * `path` - the file
///
/// # Returns
/// The text, or the size when the file is binary or too big to show.
pub fn read_file(git: &Git, rev: &str, path: &str) -> Result<FileContent> {
    let bytes = git.run_bytes(["cat-file", "blob", &format!("{rev}:{path}")])?;
    let binary = bytes.len() > MAX_TEXT_BYTES || bytes.iter().take(8000).any(|&b| b == 0);
    Ok(match (binary, String::from_utf8(bytes)) {
        (false, Ok(text)) => FileContent::Text(text),
        (_, Ok(text)) => FileContent::Binary(text.len() as u64),
        (_, Err(e)) => FileContent::Binary(e.into_bytes().len() as u64),
    })
}

/// Saves a file as it was at a commit.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `rev` - branch, tag or commit
/// * `path` - the file in the repository
/// * `dest` - where to write it
pub fn save_file(git: &Git, rev: &str, path: &str, dest: &std::path::Path) -> Result<()> {
    let bytes = git.run_bytes(["cat-file", "blob", &format!("{rev}:{path}")])?;
    std::fs::write(dest, bytes)?;
    Ok(())
}

/// Resolves a branch, tag or partial hash to a full commit hash.
///
/// # Arguments
/// * `git` - runner for the repository
/// * `rev` - what was typed
///
/// # Returns
/// The commit hash, or `None` when nothing matches.
pub fn resolve_commit(git: &Git, rev: &str) -> Option<String> {
    git.run(["rev-parse", "--verify", "-q", &format!("{}^{{commit}}", rev.trim())]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ls_tree_output_lists_folders_first() {
        let out = b"100644 blob aaa     120\tREADME.md\x00040000 tree bbb       -\tsrc\x00160000 commit ccc       -\tvendor\x00100644 blob ddd       9\tabout.txt\x00";
        let entries = parse_ls_tree(out);
        assert_eq!(entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["src", "about.txt", "README.md"]);
        assert!(entries[0].is_dir);
        assert_eq!(entries[2].size, Some(120));
    }
}
