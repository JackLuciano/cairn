use std::path::Path;

use cairn_core::Git;
use cairn_core::conflicts;
use cairn_core::graph;
use cairn_core::log::{self, LogOptions};
use cairn_core::viewed::ViewedFiles;

fn repo(dir: &Path) -> Git {
    std::fs::create_dir_all(dir).unwrap();
    let git = Git::new(dir);
    git.run(["init", "-q", "-b", "main"]).unwrap();
    git.run(["config", "user.name", "Test"]).unwrap();
    git.run(["config", "user.email", "test@example.com"]).unwrap();
    git.run(["config", "core.autocrlf", "false"]).unwrap();
    git
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn commit(git: &Git, msg: &str) {
    git.run(["add", "-A"]).unwrap();
    git.run(["commit", "-q", "-m", msg]).unwrap();
}

#[test]
fn conflicts_are_found_before_merging() {
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "one\ntwo\nthree\n");
    write(dir.path(), "b.txt", "b\n");
    commit(&git, "base");
    git.run(["switch", "-q", "-c", "clash"]).unwrap();
    write(dir.path(), "a.txt", "one\nTWO from clash\nthree\n");
    commit(&git, "clash edits a");
    git.run(["switch", "-q", "-c", "calm", "main"]).unwrap();
    write(dir.path(), "c.txt", "new\n");
    commit(&git, "calm adds c");
    git.run(["switch", "-q", "main"]).unwrap();
    write(dir.path(), "a.txt", "one\nTWO from main\nthree\n");
    commit(&git, "main edits a");

    assert_eq!(conflicts::merge_conflicts(&git, "main", "clash"), Some(vec!["a.txt".to_string()]));
    assert_eq!(conflicts::merge_conflicts(&git, "main", "calm"), Some(Vec::new()));
    assert_eq!(conflicts::merge_conflicts(&git, "main", "does-not-exist"), None);
    assert_eq!(conflicts::default_trunk(&git).as_deref(), Some("main"));
    assert_eq!(git.run(["status", "--porcelain"]).unwrap(), "", "the check leaves the working tree alone");
}

#[test]
fn history_names_the_branch_of_every_commit() {
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    git.run(["switch", "-q", "-c", "feat/x"]).unwrap();
    write(dir.path(), "x.txt", "x\n");
    commit(&git, "feature work");
    git.run(["switch", "-q", "main"]).unwrap();
    git.run(["merge", "-q", "--no-ff", "-m", "Merge branch 'feat/x'", "feat/x"]).unwrap();
    git.run(["branch", "-q", "-D", "feat/x"]).unwrap();
    write(dir.path(), "a.txt", "2\n");
    commit(&git, "after merge");

    let commits = log::load_log(&git, &LogOptions::default()).unwrap();
    let owners = graph::branch_owners(&commits);
    let by_summary = |s: &str| owners[commits.iter().position(|c| c.summary == s).unwrap()].clone();
    assert_eq!(by_summary("after merge").as_deref(), Some("main"));
    assert_eq!(by_summary("feature work").as_deref(), Some("feat/x"));
    assert_eq!(by_summary("base").as_deref(), Some("main"));
}

#[test]
fn fetch_reports_progress_while_it_runs() {
    let dir = tempfile::tempdir().unwrap();
    let origin_dir = dir.path().join("origin");
    let origin = repo(&origin_dir);
    write(&origin_dir, "a.txt", "1\n");
    commit(&origin, "base");
    let clone_dir = dir.path().join("clone");
    Git::new(dir.path()).run(["clone", "-q", origin_dir.to_str().unwrap(), clone_dir.to_str().unwrap()]).unwrap();
    for i in 0..40 {
        write(&origin_dir, &format!("f{i}.txt"), &"line\n".repeat(200 + i));
        commit(&origin, &format!("change {i}"));
    }
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, f32)>::new()));
    let sink = seen.clone();
    let git = Git::new(&clone_dir).with_progress(move |phase, done| sink.lock().unwrap().push((phase.to_string(), done)));
    git.run(["fetch", "origin"]).unwrap();
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty(), "git reported no progress");
    assert!(seen.iter().any(|(_, d)| (*d - 1.).abs() < f32::EPSILON), "a phase reached 100%: {seen:?}");
}

#[test]
fn viewed_files_reset_when_the_file_changes_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("viewed.json");
    let mut v = ViewedFiles::load_from(&path);
    v.set("C:/repo", 12, "src/a.rs", "hash-1", true);
    v.save_to(&path).unwrap();

    let v = ViewedFiles::load_from(&path);
    assert!(v.is_viewed("C:/repo", 12, "src/a.rs", "hash-1"));
    assert!(!v.is_viewed("C:/repo", 12, "src/a.rs", "hash-2"), "a new push un-ticks the file");
    assert!(!v.is_viewed("C:/repo", 13, "src/a.rs", "hash-1"));
}
