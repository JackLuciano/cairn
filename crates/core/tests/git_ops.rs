use std::collections::HashSet;
use std::path::Path;

use cairn_core::Git;
use cairn_core::absorb;
use cairn_core::conflicts::{self, Resolution};
use cairn_core::oplog::OpLog;
use cairn_core::rebase::{self, RebaseAction};

fn repo(dir: &Path) -> Git {
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

fn subjects(git: &Git) -> Vec<String> {
    git.run(["log", "--format=%s"]).unwrap().lines().map(str::to_string).collect()
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

#[test]
fn interactive_rebase_drops_rewords_and_fixes_up() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "base");
    write(dir.path(), "a.txt", "1\n2\n");
    commit(&git, "add two");
    write(dir.path(), "b.txt", "debug\n");
    commit(&git, "wip debug");
    write(dir.path(), "a.txt", "1\n2\n2b\n");
    commit(&git, "fixup! add two");
    let base = git.run(["rev-list", "--max-parents=0", "HEAD"]).unwrap().trim().to_string();
    let mut plan = rebase::plan(&git, &base, true).unwrap();
    assert_eq!(plan[1].action, RebaseAction::Fixup);
    let wip = plan.iter_mut().find(|i| i.summary == "wip debug").unwrap();
    wip.action = RebaseAction::Drop;
    plan[0].action = RebaseAction::Reword;
    plan[0].new_message = Some("feat: add two".into());
    rebase::run(&git, &base, &plan, false).unwrap();
    assert_eq!(subjects(&git), vec!["feat: add two", "base"]);
    assert!(!dir.path().join("b.txt").exists());
    assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "1\n2\n2b\n");
}

#[test]
fn absorb_folds_staged_change_into_owning_commit() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "one\n");
    write(dir.path(), "b.txt", "x\n");
    commit(&git, "base");
    write(dir.path(), "a.txt", "one\ntwo\n");
    commit(&git, "touch a");
    write(dir.path(), "b.txt", "x\ny\n");
    commit(&git, "touch b");
    write(dir.path(), "a.txt", "one\nTWO\n");
    git.run(["add", "a.txt"]).unwrap();
    let base = git.run(["rev-list", "--max-parents=0", "HEAD"]).unwrap().trim().to_string();
    let plan = absorb::plan(&git, &base).unwrap();
    assert_eq!(plan.hunks.len(), 1);
    assert_eq!(plan.hunks[0].target_summary.as_deref(), Some("touch a"));
    let n = absorb::apply(&git, &plan, &HashSet::new()).unwrap();
    assert_eq!(n, 1);
    assert_eq!(subjects(&git), vec!["touch b", "touch a", "base"]);
    let shown = git.run(["show", "HEAD~1:a.txt"]).unwrap();
    assert_eq!(shown, "one\nTWO\n");
}

#[test]
fn split_commit_into_two() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "a\n");
    write(dir.path(), "b.txt", "b\n");
    commit(&git, "base");
    write(dir.path(), "a.txt", "a\na2\n");
    write(dir.path(), "b.txt", "b\nb2\n");
    commit(&git, "both");
    let id = git.run(["rev-parse", "HEAD"]).unwrap().trim().to_string();
    absorb::split(
        &git,
        &id,
        &[
            absorb::SplitGroup { message: "only a".into(), hunks: vec![("a.txt".into(), 0)] },
            absorb::SplitGroup { message: "only b".into(), hunks: vec![("b.txt".into(), 0)] },
        ],
    )
    .unwrap();
    assert_eq!(subjects(&git), vec!["only b", "only a", "base"]);
}

#[test]
fn resolves_merge_conflict() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "f.txt", "top\nmid\nend\n");
    commit(&git, "base");
    git.run(["checkout", "-q", "-b", "feature"]).unwrap();
    write(dir.path(), "f.txt", "top\nfeature\nend\n");
    commit(&git, "feature");
    git.run(["checkout", "-q", "main"]).unwrap();
    write(dir.path(), "f.txt", "top\nmain\nend\n");
    commit(&git, "main");
    assert!(git.run(["merge", "feature"]).is_err());
    let (segments, crlf) = conflicts::load(&git, "f.txt").unwrap();
    assert_eq!(conflicts::count(&segments), 1);
    let base: Vec<String> = segments
        .iter()
        .filter_map(|s| match s {
            conflicts::Segment::Conflict(r) => Some(r.base.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(base, vec!["mid".to_string()]);
    let text = conflicts::render(&segments, &[Some(Resolution::OursThenTheirs)], crlf);
    assert!(conflicts::save(&git, "f.txt", &text).unwrap());
    assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap().replace("\r\n", "\n"), "top\nmain\nfeature\nend\n");
}

#[test]
fn stages_and_unstages_single_lines() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "f.txt", "a\nb\n");
    commit(&git, "base");
    write(dir.path(), "f.txt", "A\nb\nc\n");
    let opts = cairn_core::diff::DiffOptions::default();
    let diff = cairn_core::diff::file_diff(&git, &cairn_core::diff::DiffTarget::Unstaged, "f.txt", false, opts).unwrap();
    let c_line = diff.hunks[0].lines.iter().position(|l| l.text == "c").unwrap();
    let sel: cairn_core::patch::LineSelection = [(0, c_line)].into_iter().collect();
    cairn_core::patch::stage_lines(&git, &diff, &sel).unwrap();
    assert_eq!(git.run(["show", ":f.txt"]).unwrap(), "a\nb\nc\n");
    let staged = cairn_core::diff::file_diff(&git, &cairn_core::diff::DiffTarget::Staged, "f.txt", false, opts).unwrap();
    let c_staged = staged.hunks[0].lines.iter().position(|l| l.text == "c").unwrap();
    let sel: cairn_core::patch::LineSelection = [(0, c_staged)].into_iter().collect();
    cairn_core::patch::unstage_lines(&git, &staged, &sel).unwrap();
    assert_eq!(git.run(["show", ":f.txt"]).unwrap(), "a\nb\n");
}

#[test]
fn undo_restores_branch_after_hard_reset() {
    let _data = data_dir();
    let dir = tempfile::tempdir().unwrap();
    let git = repo(dir.path());
    write(dir.path(), "a.txt", "1\n");
    commit(&git, "one");
    write(dir.path(), "a.txt", "2\n");
    commit(&git, "two");
    let mut log = OpLog::load(&git);
    let snap = log.record(&git, "Reset").unwrap();
    git.run(["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    assert_eq!(subjects(&git), vec!["one"]);
    let mut log = OpLog::load(&git);
    log.restore(&git, &snap.id).unwrap();
    assert_eq!(subjects(&git), vec!["two", "one"]);
}
