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
fn switching_brings_a_stale_branch_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let origin_dir = dir.path().join("origin");
    let origin = repo(&origin_dir);
    write(&origin_dir, "a.txt", "1\n");
    commit(&origin, "base");
    let clone_dir = dir.path().join("clone");
    Git::new(dir.path()).run(["clone", "-q", origin_dir.to_str().unwrap(), clone_dir.to_str().unwrap()]).unwrap();
    let git = Git::new(&clone_dir);
    git.run(["config", "user.name", "Test"]).unwrap();
    git.run(["config", "user.email", "test@example.com"]).unwrap();
    git.run(["switch", "-q", "-c", "feature"]).unwrap();
    write(&origin_dir, "b.txt", "2\n");
    commit(&origin, "new on main");
    write(&origin_dir, "c.txt", "3\n");
    commit(&origin, "more on main");

    assert_eq!(cairn_core::refs::fast_forward_branch(&git, "main", true).unwrap(), 2);
    assert_eq!(git.run(["rev-parse", "main"]).unwrap(), origin.run(["rev-parse", "main"]).unwrap());
    assert_eq!(cairn_core::refs::fast_forward_branch(&git, "main", true).unwrap(), 0, "already up to date");
    assert_eq!(cairn_core::refs::fast_forward_branch(&git, "feature", true).unwrap(), 0, "no upstream");

    git.run(["switch", "-q", "main"]).unwrap();
    write(&clone_dir, "mine.txt", "x\n");
    commit(&git, "local work");
    git.run(["switch", "-q", "feature"]).unwrap();
    write(&origin_dir, "d.txt", "4\n");
    commit(&origin, "even more");
    let before = git.run(["rev-parse", "main"]).unwrap();
    assert_eq!(cairn_core::refs::fast_forward_branch(&git, "main", true).unwrap(), 0, "a branch with its own commits is left alone");
    assert_eq!(git.run(["rev-parse", "main"]).unwrap(), before);
}

#[test]
fn a_cancelled_fetch_stops_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let origin_dir = dir.path().join("origin");
    let origin = repo(&origin_dir);
    write(&origin_dir, "a.txt", "1\n");
    commit(&origin, "base");
    let clone_dir = dir.path().join("clone");
    Git::new(dir.path()).run(["clone", "-q", origin_dir.to_str().unwrap(), clone_dir.to_str().unwrap()]).unwrap();
    let handle = cairn_core::git::CancelHandle::default();
    handle.cancel();
    let git = Git::new(&clone_dir).with_progress(|_, _| {}).with_cancel(handle.clone());
    match git.run(["fetch", "origin"]) {
        Err(cairn_core::Error::Cancelled) => {}
        Ok(_) => {}
        Err(e) => panic!("expected a cancelled fetch, got {e}"),
    }
    assert!(handle.is_cancelled());
    let fresh = Git::new(&clone_dir).with_progress(|_, _| {}).with_cancel(cairn_core::git::CancelHandle::default());
    fresh.run(["fetch", "origin"]).unwrap();
}

#[test]
fn sync_plan_spots_a_rewritten_server_branch() {
    let dir = tempfile::tempdir().unwrap();
    let origin_dir = dir.path().join("origin");
    let origin = repo(&origin_dir);
    write(&origin_dir, "a.txt", "1\n");
    commit(&origin, "base");
    origin.run(["config", "receive.denyCurrentBranch", "ignore"]).unwrap();
    let mine_dir = dir.path().join("mine");
    let theirs_dir = dir.path().join("theirs");
    for d in [&mine_dir, &theirs_dir] {
        Git::new(dir.path()).run(["clone", "-q", origin_dir.to_str().unwrap(), d.to_str().unwrap()]).unwrap();
        let g = Git::new(d);
        g.run(["config", "user.name", "Test"]).unwrap();
        g.run(["config", "user.email", "test@example.com"]).unwrap();
    }
    let mine = Git::new(&mine_dir);
    let theirs = Git::new(&theirs_dir);
    write(&theirs_dir, "b.txt", "b\n");
    commit(&theirs, "shared work");
    theirs.run(["push", "-q", "origin", "main"]).unwrap();
    mine.run(["pull", "-q", "--ff-only"]).unwrap();
    assert_eq!(cairn_core::ops::sync_plan(&mine), cairn_core::ops::SyncPlan::default());

    write(&mine_dir, "c.txt", "c\n");
    commit(&mine, "my work");
    let plan = cairn_core::ops::sync_plan(&mine);
    assert_eq!((plan.ahead, plan.behind, plan.rewritten), (1, 0, 0));

    write(&theirs_dir, "z.txt", "z\n");
    commit(&theirs, "first on server");
    theirs.run(["reset", "-q", "--hard", "HEAD~2"]).unwrap();
    write(&theirs_dir, "z.txt", "z\n");
    commit(&theirs, "first on server");
    write(&theirs_dir, "b.txt", "b\n");
    commit(&theirs, "shared work");
    theirs.run(["push", "-q", "--force", "origin", "main"]).unwrap();
    mine.run(["fetch", "-q"]).unwrap();
    let plan = cairn_core::ops::sync_plan(&mine);
    assert_eq!((plan.ahead, plan.behind), (2, 2));
    assert_eq!(plan.rewritten, 1, "the shared commit came back with a new id");
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
