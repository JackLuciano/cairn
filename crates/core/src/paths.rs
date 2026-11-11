use std::path::{Path, PathBuf};

/// The folder Cairn keeps its settings and data in (`%APPDATA%\Cairn` on Windows).
///
/// # Returns
/// The data directory; it is created if missing.
pub fn data_dir() -> PathBuf {
    let dir = std::env::var_os("CAIRN_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|d| d.join("Cairn")))
        .unwrap_or_else(|| PathBuf::from(".cairn-data"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Writes a temporary file with a name no other operation shares, so rebases and conflict
/// lookups in different repositories never overwrite each other's files. Temporary files older
/// than a week are removed along the way.
///
/// # Arguments
/// * `name` - readable suffix for the file name
/// * `contents` - file contents
///
/// # Returns
/// The path of the new file.
pub fn temp_file(name: &str, contents: &[u8]) -> std::io::Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = data_dir().join("tmp");
    std::fs::create_dir_all(&dir)?;
    let week = std::time::Duration::from_secs(7 * 86_400);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > week);
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    let path = dir.join(format!("{}-{n}-{safe}", std::process::id()));
    std::fs::write(&path, contents)?;
    Ok(path)
}

/// A stable short identifier for a repository path, used for per-repo data files.
///
/// # Arguments
/// * `path` - repository root
///
/// # Returns
/// A 16-character hex string.
pub fn repo_key(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = canonical.to_string_lossy().to_lowercase().replace('\\', "/");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Writes a file atomically by writing a temporary file and renaming it.
///
/// # Arguments
/// * `path` - destination file
/// * `bytes` - file contents
///
/// # Returns
/// `Ok(())` when the file is in place.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
