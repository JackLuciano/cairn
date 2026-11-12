use std::path::Path;

use cairn_core::Git;
use cairn_core::compare;
use cairn_core::diff::{self, DiffOptions};
use cairn_core::log;
use cairn_core::ops;
use cairn_core::recover::{self, LostKind};
use cairn_core::refs;
use cairn_core::submodule;

fn repo(dir: &Path) -> Git {
    std::fs::create_dir_all(dir).unwrap();
    let git = Git::new(dir);
    git.run(["init", "-q", "-b", "main"]).unwrap();
    git.run(["config", "user.name", "Test"]).unwrap();
    git.run(["config", "user.email", "test@example.com"]).unwrap();
    git.run(["config", "core.autocrlf", "false"]).unwrap();
    git.run(["config", "protocol.file.allow", "always"]).unwrap();
    git
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn commit(git: &Git, msg: &str) -> String {
    git.run(["add", "-A"]).unwrap();
    git.run(["commit", "-q", "-m", msg]).unwrap();
    git.run(["rev-parse", "HEAD"]).unwrap().trim().to_string()
}

#[test]
fn file_history_follows_renames_and_shows_each_diff() {
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "src/old.rs", "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n");
    commit(&git, "add");
    write(dir.path(), "other.txt", "x");
    commit(&git, "unrelated");
    git.run(["mv", "src/old.rs", "src/new.rs"]).unwrap();
    write(dir.path(), "src/new.rs", "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n");
    let renamed = commit(&git, "rename and grow");

    let history = log::file_history(&git, "src/new.rs", 100).unwrap();
    assert_eq!(history.iter().map(|v| v.summary.as_str()).collect::<Vec<_>>(), vec!["rename and grow", "add"]);
    assert_eq!(history[0].status, 'R');
    assert_eq!(history[0].old_path.as_deref(), Some("src/old.rs"));
    assert_eq!(history[1].path, "src/old.rs");

    let v = &history[0];
    let d = diff::revision_file_diff(&git, v.parent.as_deref(), &renamed, &v.path, v.old_path.as_deref(), DiffOptions::default()).unwrap();
    assert_eq!(d.stats(), (1, 0));
    let first = &history[1];
    let d = diff::revision_file_diff(&git, first.parent.as_deref(), &first.id, &first.path, None, DiffOptions::default()).unwrap();
    assert_eq!(d.stats(), (4, 0));
}

#[test]
fn compare_lists_both_sides_and_changed_files() {
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    git.run(["checkout", "-q", "-b", "feature"]).unwrap();
    write(dir.path(), "b.txt", "new\n");
    commit(&git, "feature one");
    write(dir.path(), "a.txt", "1\n2\n");
    commit(&git, "feature two");
    git.run(["checkout", "-q", "main"]).unwrap();
    write(dir.path(), "c.txt", "main\n");
    commit(&git, "main moved");

    let c = compare::compare(&git, "main", "feature", 100).unwrap();
    assert_eq!(c.ahead.iter().map(|x| x.summary.as_str()).collect::<Vec<_>>(), vec!["feature two", "feature one"]);
    assert_eq!(c.behind.iter().map(|x| x.summary.as_str()).collect::<Vec<_>>(), vec!["main moved"]);
    let mut paths: Vec<&str> = c.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, vec!["a.txt", "b.txt"]);
    assert!(compare::compare(&git, "main", "no-such-branch", 10).is_err());
}

#[test]
fn lost_commits_and_head_history_find_deleted_work() {
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    git.run(["checkout", "-q", "-b", "doomed"]).unwrap();
    write(dir.path(), "a.txt", "2\n");
    let lost = commit(&git, "work on doomed");
    git.run(["checkout", "-q", "main"]).unwrap();
    refs::delete_branch(&git, "doomed", true).unwrap();
    write(dir.path(), "a.txt", "stash me\n");
    refs::stash_push(&git, Some("half done"), false).unwrap();
    refs::stash_drop(&git, 0).unwrap();

    let found = recover::lost_commits(&git).unwrap();
    assert!(found.iter().any(|c| c.id == lost && c.kind == LostKind::Dangling));
    assert!(found.iter().any(|c| c.kind == LostKind::DroppedStash && c.summary.contains("half done")));

    let moves = recover::head_history(&git, 50).unwrap();
    let entry = moves.iter().find(|c| c.id == lost).expect("reflog has the lost commit");
    assert!(!entry.reachable);
    assert!(moves.iter().any(|c| c.reachable));

    refs::create_branch(&git, "rescued", &lost, false).unwrap();
    assert_eq!(git.run(["rev-parse", "rescued"]).unwrap().trim(), lost);
}

#[test]
fn partial_and_staged_stashes_and_single_file_restore() {
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "a\n");
    write(dir.path(), "b.txt", "b\n");
    commit(&git, "base");
    write(dir.path(), "a.txt", "a changed\n");
    write(dir.path(), "b.txt", "b changed\n");
    write(dir.path(), "new.txt", "brand new\n");

    refs::stash_push_paths(&git, Some("just a"), &["a.txt".into(), "new.txt".into()], true).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "a\n");
    assert_eq!(std::fs::read_to_string(dir.path().join("b.txt")).unwrap(), "b changed\n");
    assert!(!dir.path().join("new.txt").exists());

    let stash = refs::list_stashes(&git).unwrap().remove(0);
    let files = refs::stash_files(&git, &stash.id).unwrap();
    assert!(files.iter().any(|f| f.path == "a.txt" && !f.untracked));
    assert!(files.iter().any(|f| f.path == "new.txt" && f.untracked));

    refs::stash_restore_file(&git, &stash.id, "new.txt", true).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("new.txt")).unwrap(), "brand new\n");
    let d = diff::revision_file_diff(&git, Some(&format!("{}^1", stash.id)), &stash.id, "a.txt", None, DiffOptions::default()).unwrap();
    assert_eq!(d.stats(), (1, 1));

    git.run(["add", "b.txt"]).unwrap();
    refs::stash_push_staged(&git, Some("staged only")).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("b.txt")).unwrap(), "b\n");
    assert_eq!(refs::list_stashes(&git).unwrap().len(), 2);
}

#[test]
fn patches_round_trip_between_repositories() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let ga = repo(&a);
    write(&a, "f.txt", "1\n");
    commit(&ga, "base");
    Git::new(dir.path()).run(["clone", "-q", "-c", "core.autocrlf=false", a.to_str().unwrap(), b.to_str().unwrap()]).unwrap();
    let gb = Git::new(&b);
    gb.run(["config", "user.name", "Test"]).unwrap();
    gb.run(["config", "user.email", "test@example.com"]).unwrap();
    write(&a, "f.txt", "1\n2\n");
    let one = commit(&ga, "two");
    write(&a, "f.txt", "1\n2\n3\n");
    let two = commit(&ga, "three");

    let file = dir.path().join("work.patch");
    ops::save_patch(&ga, &[two, one], &file).unwrap();
    assert!(ops::is_mailbox_patch(&std::fs::read_to_string(&file).unwrap()));
    ops::apply_patch(&gb, &file).unwrap();
    assert_eq!(gb.run(["log", "--format=%s", "-2"]).unwrap().lines().collect::<Vec<_>>(), vec!["three", "two"]);

    let plain = dir.path().join("plain.diff");
    write(&a, "f.txt", "changed\n");
    std::fs::write(&plain, ga.run(["diff"]).unwrap()).unwrap();
    assert!(!ops::is_mailbox_patch(&std::fs::read_to_string(&plain).unwrap()));
    ops::apply_patch(&gb, &plain).unwrap();
    assert_eq!(std::fs::read_to_string(b.join("f.txt")).unwrap(), "changed\n");
}

#[test]
fn submodules_are_listed_and_updated() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let lg = repo(&lib);
    write(&lib, "lib.txt", "lib\n");
    commit(&lg, "lib");
    let app = dir.path().join("app");
    let ag = repo(&app);
    write(&app, "app.txt", "app\n");
    commit(&ag, "app");
    ag.run(["-c", "protocol.file.allow=always", "submodule", "add", "-q", lib.to_str().unwrap(), "vendor/lib"]).unwrap();
    commit(&ag, "add lib");

    let list = submodule::list(&ag).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].path, "vendor/lib");
    assert_eq!(list[0].state, submodule::SubmoduleState::UpToDate);
    assert!(list[0].url.ends_with("lib"));

    let clone = dir.path().join("clone");
    Git::new(dir.path()).run(["clone", "-q", app.to_str().unwrap(), clone.to_str().unwrap()]).unwrap();
    let cg = Git::new(&clone);
    cg.run(["config", "protocol.file.allow", "always"]).unwrap();
    assert_eq!(submodule::list(&cg).unwrap()[0].state, submodule::SubmoduleState::NotInitialized);
    submodule::update(&cg.clone().with_env("GIT_ALLOW_PROTOCOL", "file"), None).unwrap();
    assert_eq!(submodule::list(&cg).unwrap()[0].state, submodule::SubmoduleState::UpToDate);
    assert!(clone.join("vendor/lib/lib.txt").exists());
}
