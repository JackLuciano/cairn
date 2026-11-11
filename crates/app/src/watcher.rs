use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub(crate) struct RepoWatcher {
    _watcher: RecommendedWatcher,
    pub rx: smol::channel::Receiver<()>,
}

fn relevant(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return true };
    let mut parts = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned());
    match parts.next().as_deref() {
        Some(".git") => {
            let rest: Vec<String> = parts.collect();
            let joined = rest.join("/");
            joined == "index"
                || joined == "HEAD"
                || joined.starts_with("refs/")
                || joined == "MERGE_HEAD"
                || joined == "packed-refs"
                || joined.starts_with("rebase-merge")
                || joined.starts_with("rebase-apply")
        }
        Some("node_modules") | Some("target") => false,
        _ => true,
    }
}

impl RepoWatcher {
    pub(crate) fn start(root: PathBuf) -> Option<Self> {
        let (tx, rx) = smol::channel::bounded::<()>(1);
        let root_cb = root.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && event.paths.iter().any(|p| relevant(&root_cb, p))
            {
                let _ = tx.try_send(());
            }
        })
        .ok()?;
        watcher.watch(&root, RecursiveMode::Recursive).ok()?;
        Some(Self { _watcher: watcher, rx })
    }
}

pub(crate) const DEBOUNCE: Duration = Duration::from_millis(400);
