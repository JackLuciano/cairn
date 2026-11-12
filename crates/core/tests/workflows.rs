use std::path::{Path, PathBuf};
use std::time::Duration;

use cairn_core::Git;
use cairn_core::bisect;
use cairn_core::blame;
use cairn_core::checks::{self, CheckDef, CheckStatus, Stage};
use cairn_core::ops::{self, CommitOptions, ForceMode, PullMode, ResetMode};
use cairn_core::refs;
use cairn_core::stack;
use cairn_core::status::load_status;
use cairn_core::workspace;

fn repo(dir: &Path) -> Git {
    let git = Git::new(dir);
    git.run(["init", "-q", "-b", "main"]).unwrap();
    git.run(["config", "user.name", "Test"]).unwrap();
    git.run(["config", "user.email", "test@example.com"]).unwrap();
    git.run(["config", "core.autocrlf", "false"]).unwrap();
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
    head(git)
}

fn head(git: &Git) -> String {
    git.run(["rev-parse", "HEAD"]).unwrap().trim().to_string()
}

fn subjects(git: &Git, range: &str) -> Vec<String> {
    git.run(["log", "--format=%s", range]).unwrap().lines().map(str::to_string).collect()
}

fn data_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let d = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("CAIRN_DATA_DIR", d.path()) };
        d
    })
    .path()
}

/// Builds a bare "origin" plus a clone of it with one pushed commit.
fn with_remote() -> (tempfile::TempDir, PathBuf, Git) {
    let root = tempfile::tempdir().unwrap();
    let bare = root.path().join("origin.git");
    Git::new(root.path()).run(["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]).unwrap();
    let work = root.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let git = repo(&work);
    git.run(["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
    write(&work, "readme.md", "hello\n");
    commit(&git, "initial");
    ops::push(&git, "origin", "main", true, ForceMode::None, false).unwrap();
    (root, bare, git)
}

fn clone_of(root: &Path, bare: &Path, name: &str) -> Git {
    let path = root.join(name);
    Git::new(root).run(["clone", "-q", bare.to_str().unwrap(), path.to_str().unwrap()]).unwrap();
    let git = Git::new(&path);
    git.run(["config", "user.name", "Other"]).unwrap();
    git.run(["config", "user.email", "other@example.com"]).unwrap();
    git.run(["config", "core.autocrlf", "false"]).unwrap();
    git
}

#[test]
fn branch_lifecycle_and_validation() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    let first = commit(&git, "base");

    refs::create_branch(&git, "feat/x", "HEAD", true).unwrap();
    assert_eq!(load_status(&git).unwrap().branch.head.as_deref(), Some("feat/x"));
    write(dir.path(), "a.txt", "1\n2\n");
    commit(&git, "on feature");

    refs::rename_branch(&git, "feat/x", "feat/y").unwrap();
    let names: Vec<String> = refs::list_branches(&git).unwrap().into_iter().filter(|b| !b.is_remote).map(|b| b.name).collect();
    assert!(names.contains(&"feat/y".to_string()) && !names.contains(&"feat/x".to_string()));

    refs::checkout(&git, "main").unwrap();
    assert!(refs::delete_branch(&git, "feat/y", false).is_err());
    refs::delete_branch(&git, "feat/y", true).unwrap();
    assert_eq!(refs::list_branches(&git).unwrap().iter().filter(|b| !b.is_remote).count(), 1);

    refs::create_tag(&git, "v1.0.0", &first, Some("First")).unwrap();
    assert_eq!(refs::list_tags(&git).unwrap()[0].name, "v1.0.0");
    refs::delete_tag(&git, "v1.0.0").unwrap();
    assert!(refs::list_tags(&git).unwrap().is_empty());

    assert!(refs::is_valid_branch_name(&git, "fix/login-timeout"));
    assert!(!refs::is_valid_branch_name(&git, "bad..name"));
    assert!(!refs::is_valid_branch_name(&git, "  "));
}

#[test]
fn stash_round_trip_keeps_untracked_files() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    write(dir.path(), "a.txt", "changed\n");
    write(dir.path(), "new.txt", "untracked\n");

    refs::stash_push(&git, Some("half done"), true).unwrap();
    assert!(load_status(&git).unwrap().is_clean());
    let stashes = refs::list_stashes(&git).unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("half done"));

    refs::stash_apply(&git, 0, true).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "changed\n");
    assert!(dir.path().join("new.txt").exists());
    assert!(refs::list_stashes(&git).unwrap().is_empty());
}

#[test]
fn commit_amend_and_reset_modes() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    let base = commit(&git, "base");
    write(dir.path(), "a.txt", "1\n2\n");
    ops::stage_all(&git).unwrap();
    ops::commit(&git, "second\n\nwith a body", &CommitOptions::default()).unwrap();
    assert_eq!(ops::head_message(&git).trim(), "second\n\nwith a body");

    write(dir.path(), "b.txt", "late\n");
    ops::stage_files(&git, &["b.txt".into()]).unwrap();
    ops::commit(&git, "second, amended", &CommitOptions { amend: true, ..Default::default() }).unwrap();
    assert_eq!(subjects(&git, "HEAD"), ["second, amended", "base"]);

    ops::reset(&git, &base, ResetMode::Soft).unwrap();
    assert_eq!(load_status(&git).unwrap().staged().count(), 2);
    ops::unstage_all(&git).unwrap();
    assert_eq!(load_status(&git).unwrap().staged().count(), 0);
    ops::reset(&git, &base, ResetMode::Hard).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "1\n");
}

#[test]
fn discard_restores_tracked_and_removes_untracked() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    write(dir.path(), "a.txt", "oops\n");
    write(dir.path(), "junk.txt", "x\n");
    ops::discard_files(&git, &["a.txt".into()], &["junk.txt".into()]).unwrap();
    assert!(load_status(&git).unwrap().is_clean());
    assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "1\n");
}

#[test]
fn cherry_pick_and_revert() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    refs::create_branch(&git, "side", "HEAD", true).unwrap();
    write(dir.path(), "side.txt", "side\n");
    let picked = commit(&git, "side work");
    refs::checkout(&git, "main").unwrap();

    ops::cherry_pick(&git, &picked).unwrap();
    assert!(dir.path().join("side.txt").exists());
    let main_copy = head(&git);
    ops::revert(&git, &main_copy).unwrap();
    assert!(!dir.path().join("side.txt").exists());
    assert_eq!(subjects(&git, "HEAD").len(), 3);
}

#[test]
fn cherry_pick_and_revert_many_keep_history_order() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    refs::create_branch(&git, "side", "HEAD", true).unwrap();
    write(dir.path(), "one.txt", "1\n");
    let one = commit(&git, "one");
    write(dir.path(), "one.txt", "1\n2\n");
    let two = commit(&git, "two");
    write(dir.path(), "three.txt", "3\n");
    let three = commit(&git, "three");
    refs::checkout(&git, "main").unwrap();

    let scrambled = vec![three.clone(), one.clone(), two.clone()];
    assert_eq!(ops::oldest_first(&git, &scrambled), vec![one.clone(), two.clone(), three.clone()]);
    ops::cherry_pick_many(&git, &scrambled).unwrap();
    assert_eq!(subjects(&git, "HEAD"), ["three", "two", "one", "base"]);

    let picked: Vec<String> = git.run(["rev-list", "-3", "HEAD"]).unwrap().lines().map(str::to_string).collect();
    ops::revert_many(&git, &picked).unwrap();
    assert!(!dir.path().join("one.txt").exists());
    assert!(!dir.path().join("three.txt").exists());
    assert_eq!(subjects(&git, "HEAD").len(), 7);
}

#[test]
fn push_fetch_pull_round_trip() {
    let _data = data_dir();
    let (root, bare, git) = with_remote();
    let other = clone_of(root.path(), &bare, "other");
    write(other.dir(), "other.txt", "from the other side\n");
    commit(&other, "other change");
    ops::push(&other, "origin", "main", false, ForceMode::None, false).unwrap();

    ops::fetch(&git, None, true).unwrap();
    let status = load_status(&git).unwrap();
    assert_eq!(status.branch.behind, 1);
    assert_eq!(status.branch.ahead, 0);

    write(git.dir(), "mine.txt", "local\n");
    commit(&git, "local change");
    ops::pull(&git, PullMode::Rebase, true).unwrap();
    assert_eq!(subjects(&git, "HEAD"), ["local change", "other change", "initial"]);
    let status = load_status(&git).unwrap();
    assert_eq!((status.branch.ahead, status.branch.behind), (1, 0));

    ops::push(&git, "origin", "main", false, ForceMode::None, false).unwrap();
    assert_eq!(load_status(&git).unwrap().branch.ahead, 0);
}

#[test]
fn safe_force_push_rewrites_own_work_but_never_unseen_pushes() {
    let _data = data_dir();
    let (root, bare, git) = with_remote();

    git.run(["commit", "-q", "--amend", "-m", "initial, reworded"]).unwrap();
    assert!(ops::push(&git, "origin", "main", false, ForceMode::None, false).is_err());
    ops::push(&git, "origin", "main", false, ForceMode::WithLease, false).unwrap();

    let other = clone_of(root.path(), &bare, "other");
    write(other.dir(), "theirs.txt", "x\n");
    commit(&other, "their push");
    ops::push(&other, "origin", "main", false, ForceMode::None, false).unwrap();

    git.run(["commit", "-q", "--amend", "-m", "initial, reworded again"]).unwrap();
    assert!(ops::push(&git, "origin", "main", false, ForceMode::WithLease, false).is_err());
    ops::fetch(&git, None, false).unwrap();
    assert!(ops::push(&git, "origin", "main", false, ForceMode::WithLease, false).is_err());
    let theirs = clone_of(root.path(), &bare, "check");
    assert_eq!(subjects(&theirs, "HEAD")[0], "their push");
}

#[test]
fn fast_forward_only_pull_refuses_diverged_history() {
    let _data = data_dir();
    let (root, bare, git) = with_remote();
    let other = clone_of(root.path(), &bare, "other");
    write(other.dir(), "theirs.txt", "x\n");
    commit(&other, "theirs");
    ops::push(&other, "origin", "main", false, ForceMode::None, false).unwrap();
    write(git.dir(), "mine.txt", "y\n");
    commit(&git, "mine");
    assert!(ops::pull(&git, PullMode::FastForwardOnly, false).is_err());
    ops::pull(&git, PullMode::Merge, false).unwrap();
    assert!(git.dir().join("theirs.txt").exists());
}

#[test]
fn bisect_finds_the_first_bad_commit() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "value.txt", "ok\n");
    let good = commit(&git, "c0");
    for i in 1..=3 {
        write(dir.path(), &format!("f{i}.txt"), "x\n");
        commit(&git, &format!("c{i}"));
    }
    write(dir.path(), "value.txt", "broken\n");
    let culprit = commit(&git, "c4 breaks it");
    for i in 5..=7 {
        write(dir.path(), &format!("f{i}.txt"), "x\n");
        commit(&git, &format!("c{i}"));
    }
    let bad = head(&git);

    bisect::start(&git, &bad, &good).unwrap();
    let mut found = None;
    for _ in 0..10 {
        let state = bisect::state(&git).unwrap();
        assert!(state.active);
        if let Some(f) = state.first_bad {
            found = Some(f);
            break;
        }
        let broken = std::fs::read_to_string(dir.path().join("value.txt")).unwrap().contains("broken");
        let out = bisect::mark(&git, if broken { "bad" } else { "good" }).unwrap();
        if let Some(f) = bisect::first_bad_from_output(&out) {
            found = Some(f);
            break;
        }
    }
    assert!(culprit.starts_with(found.as_deref().unwrap()) || found.as_deref() == Some(culprit.as_str()));
    bisect::reset(&git).unwrap();
    assert!(!bisect::state(&git).unwrap().active);
    assert_eq!(head(&git), bad);
}

#[test]
fn blame_and_line_history_follow_edits() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.lua", "local a = 1\nlocal b = 2\n");
    let first = commit(&git, "add a and b");
    write(dir.path(), "a.lua", "local a = 1\nlocal b = 3\n");
    let second = commit(&git, "bump b");
    write(dir.path(), "a.lua", "local a = 1\nlocal b = 4\n");
    let third = commit(&git, "bump b again");

    let lines = blame::blame(&git, "a.lua", Some("HEAD")).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].commit, first);
    assert_eq!(lines[1].commit, third);
    assert_eq!(lines[1].summary, "bump b again");

    let history = blame::line_history(&git, "a.lua", 2, 2).unwrap();
    let ids: Vec<&str> = history.iter().map(|v| v.commit.as_str()).collect();
    assert_eq!(ids, [third.as_str(), second.as_str(), first.as_str()]);
    assert!(history[0].text.iter().any(|l| l.contains("= 4")));
}

#[test]
fn checks_load_run_and_time_out() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(
        dir.path(),
        ".cairn/checks.toml",
        "[[check]]\nname = \"ok\"\nrun = \"git --version\"\non = [\"commit\", \"push\"]\n\n[[check]]\nname = \"warn\"\nrun = \"git definitely-not-a-command\"\nblocking = false\n",
    );
    let defs = checks::load(&git, ".cairn/checks.toml").unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(defs[0].on, vec![Stage::Commit, Stage::Push]);
    assert_eq!(defs[1].on, vec![Stage::Push]);

    let ok = checks::run_check(&git, &defs[0], Duration::from_secs(30));
    assert_eq!(ok.status, CheckStatus::Passed);
    assert!(ok.output.contains("git version"));
    let failed = checks::run_check(&git, &defs[1], Duration::from_secs(30));
    assert_eq!(failed.status, CheckStatus::Failed);
    assert!(!failed.blocking);

    let slow = CheckDef {
        name: "slow".into(),
        run: if cfg!(windows) { "ping -n 6 127.0.0.1 > nul".into() } else { "sleep 5".into() },
        on: vec![Stage::Push],
        blocking: Some(true),
    };
    let timed_out = checks::run_check(&git, &slow, Duration::from_millis(500));
    assert_eq!(timed_out.status, CheckStatus::TimedOut);
}

#[test]
fn secret_scan_blocks_staged_keys_until_allowed() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    let key = format!("sk-ant-{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4");
    write(dir.path(), "config.lua", &format!("Config.key = '{key}'\n"));
    ops::stage_all(&git).unwrap();

    let found = checks::scan_staged(&git).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, "Anthropic API key");
    assert_eq!(found[0].file, "config.lua");
    assert!(!found[0].masked.contains(&key));
    assert!(!checks::redact(&format!("x {key} y")).contains(&key));

    checks::allow(&git, &found[0].fingerprint).unwrap();
    assert!(checks::allow_list(&git).contains(&found[0].fingerprint));
    let diff = git.run(["diff", "--cached"]).unwrap();
    assert!(checks::scan_diff(&diff, &checks::allow_list(&git)).is_empty());
}

#[test]
fn worktrees_are_added_listed_and_pruned_once_merged() {
    let _data = data_dir();
    let root = tempfile::tempdir().unwrap();
    let main_dir = root.path().join("main");
    std::fs::create_dir_all(&main_dir).unwrap();
    let git = repo(&main_dir);
    write(&main_dir, "a.txt", "1\n");
    commit(&git, "base");

    let wt_path = root.path().join("wt-fix");
    refs::add_worktree(&git, &wt_path, "fix/typo").unwrap();
    let listed = refs::list_worktrees(&git).unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|w| w.is_main));
    assert!(listed.iter().any(|w| w.branch.as_deref() == Some("fix/typo")));

    let wt = Git::new(&wt_path);
    wt.run(["config", "user.name", "Test"]).unwrap();
    write(&wt_path, "b.txt", "fix\n");
    commit(&wt, "fix typo");
    assert!(refs::prune_merged_worktrees(&git, "main", 0).unwrap().is_empty());

    ops::merge(&git, "fix/typo", false).unwrap();
    assert!(refs::prune_merged_worktrees(&git, "main", 3650).unwrap().is_empty());
    let removed = refs::prune_merged_worktrees(&git, "main", 0).unwrap();
    assert_eq!(removed.len(), 1);
    assert!(!wt_path.exists());
}

#[test]
fn worktree_with_uncommitted_changes_is_never_pruned() {
    let _data = data_dir();
    let root = tempfile::tempdir().unwrap();
    let main_dir = root.path().join("main");
    std::fs::create_dir_all(&main_dir).unwrap();
    let git = repo(&main_dir);
    write(&main_dir, "a.txt", "1\n");
    commit(&git, "base");
    let wt_path = root.path().join("wt-dirty");
    refs::add_worktree(&git, &wt_path, "dirty").unwrap();
    write(&wt_path, "a.txt", "unsaved work\n");
    assert!(refs::prune_merged_worktrees(&git, "main", 0).unwrap().is_empty());
    assert!(wt_path.exists());
}

#[test]
fn workspace_links_same_named_branches_and_commits_together() {
    let _data = data_dir();
    let root = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for name in ["api", "ui", "docs"] {
        let p = root.path().join(name);
        std::fs::create_dir_all(&p).unwrap();
        let git = repo(&p);
        write(&p, "readme.md", name);
        commit(&git, "base");
        paths.push(p);
    }
    let linked_paths = paths[..2].to_vec();
    for out in workspace::switch_all(&linked_paths, "feat/login", true) {
        assert!(out.ok, "{}", out.message);
    }
    let summaries: Vec<_> = paths.iter().map(|p| workspace::summarize(p)).collect();
    let linked = workspace::linked_branches(&summaries, "main");
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].0, "feat/login");
    assert_eq!(linked[0].1, linked_paths);

    for p in &linked_paths {
        write(p, "login.txt", "login\n");
        ops::stage_all(&Git::new(p)).unwrap();
    }
    for out in workspace::commit_all(&linked_paths, "feat: login", false) {
        assert!(out.ok, "{}", out.message);
    }
    for p in &linked_paths {
        assert_eq!(subjects(&Git::new(p), "HEAD")[0], "feat: login");
        assert_eq!(workspace::summarize(p).changed, 0);
    }
    assert_eq!(subjects(&Git::new(&paths[2]), "HEAD"), ["base"]);
}

#[test]
fn stack_detection_and_restack() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    refs::create_branch(&git, "part-1", "HEAD", true).unwrap();
    write(dir.path(), "one.txt", "1\n");
    commit(&git, "part one");
    refs::create_branch(&git, "part-2", "HEAD", true).unwrap();
    write(dir.path(), "two.txt", "2\n");
    commit(&git, "part two");

    let found = stack::detect(&git, "main", &[]).unwrap();
    assert_eq!(found.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["part-1", "part-2"]);
    assert_eq!(found[0].base, "main");
    assert_eq!(found[1].base, "part-1");
    assert!(found.iter().all(|b| !b.needs_restack && b.commits.len() == 1));

    refs::checkout(&git, "part-1").unwrap();
    git.run(["commit", "-q", "--amend", "-m", "part one, reviewed"]).unwrap();
    refs::checkout(&git, "part-2").unwrap();
    let moved = stack::detect(&git, "main", &[]).unwrap();
    assert!(moved.iter().find(|b| b.name == "part-2").unwrap().needs_restack);

    refs::checkout(&git, "main").unwrap();
    write(dir.path(), "trunk.txt", "t\n");
    commit(&git, "trunk moved");
    stack::restack(&git, "main", "part-2").unwrap();
    assert_eq!(subjects(&git, "part-2"), ["part two", "part one, reviewed", "trunk moved", "base"]);
    assert_eq!(subjects(&git, "part-1")[0], "part one, reviewed");
    let after = stack::detect(&git, "main", &[]).unwrap();
    assert!(after.iter().all(|b| !b.needs_restack));
}

#[test]
fn branch_base_covers_pushed_feature_work_and_stack_parents() {
    let _data = data_dir();
    let (_root, _bare, git) = with_remote();
    let repo = cairn_core::repo::Repository::open(git.dir()).unwrap();
    let main_tip = head(&git);

    refs::create_branch(&git, "feat/api", "HEAD", true).unwrap();
    write(git.dir(), "api.txt", "1\n");
    commit(&git, "api work");
    ops::push(&git, "origin", "feat/api", true, ForceMode::None, false).unwrap();
    let (label, base) = repo.branch_base().unwrap();
    assert!(label.ends_with("main"), "{label}");
    assert_eq!(base, main_tip);

    refs::create_branch(&git, "feat/ui", "HEAD", true).unwrap();
    write(git.dir(), "ui.txt", "1\n");
    commit(&git, "ui work");
    let api_tip = git.run(["rev-parse", "feat/api"]).unwrap().trim().to_string();
    assert_eq!(repo.branch_base().unwrap(), ("feat/api".to_string(), api_tip));

    refs::checkout(&git, "main").unwrap();
    write(git.dir(), "local.txt", "1\n");
    commit(&git, "unpushed on main");
    let (label, base) = repo.branch_base().unwrap();
    assert_eq!(label, "origin/main");
    assert_eq!(base, main_tip);
}

#[test]
fn stack_survives_amend_and_rename_without_prior_detection() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    refs::create_branch(&git, "lower", "HEAD", true).unwrap();
    write(dir.path(), "one.txt", "1\n");
    commit(&git, "lower work");
    refs::create_branch(&git, "upper", "HEAD", true).unwrap();
    write(dir.path(), "two.txt", "2\n");
    commit(&git, "upper work");

    refs::checkout(&git, "lower").unwrap();
    git.run(["commit", "-q", "--amend", "-m", "lower work, amended"]).unwrap();
    refs::rename_branch(&git, "lower", "bottom").unwrap();

    refs::checkout(&git, "upper").unwrap();
    let found = stack::detect(&git, "main", &[]).unwrap();
    assert_eq!(found.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["bottom", "upper"]);
    let upper = &found[1];
    assert_eq!(upper.base, "bottom");
    assert!(upper.needs_restack);
    assert_eq!(upper.commits.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), ["upper work"]);

    stack::restack(&git, "main", "upper").unwrap();
    assert_eq!(subjects(&git, "upper"), ["upper work", "lower work, amended", "base"]);
    assert_eq!(load_status(&git).unwrap().branch.head.as_deref(), Some("upper"));
}
