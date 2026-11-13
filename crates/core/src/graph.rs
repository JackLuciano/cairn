use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::log::{CommitInfo, RefKind};

/// Which part of a row a graph line occupies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinePart {
    /// A vertical line through the whole row, for a lane that does not touch this commit.
    Through,
    /// From the top of the row at `from` into the commit dot.
    Top,
    /// From the commit dot down to the bottom of the row at `to`.
    Bottom,
}

/// One line segment drawn in a graph row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphLine {
    pub from: usize,
    pub to: usize,
    pub part: LinePart,
    pub color: usize,
    pub dim: bool,
}

/// Graph drawing data for one commit row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    pub lines: Vec<GraphLine>,
    pub width: usize,
    pub dim: bool,
}

/// Marks which loaded commits are reachable from a tip commit.
///
/// # Arguments
/// * `commits` - commits in topological order, newest first
/// * `tip` - the commit to start from, such as the checked-out commit
///
/// # Returns
/// One flag per commit, or `None` when the tip is not among the loaded commits.
pub fn reachable(commits: &[CommitInfo], tip: &str) -> Option<Vec<bool>> {
    let start = commits.iter().position(|c| c.id == tip)?;
    let index: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let mut hot = vec![false; commits.len()];
    hot[start] = true;
    for i in start..commits.len() {
        if !hot[i] {
            continue;
        }
        for p in &commits[i].parents {
            if let Some(&j) = index.get(p.as_str()) {
                hot[j] = true;
            }
        }
    }
    Some(hot)
}

fn trunk_rank(name: &str) -> u8 {
    let short = name.rsplit_once('/').filter(|(remote, _)| !remote.contains('/') && !matches!(*remote, "release" | "releases" | "hotfix")).map(|(_, b)| b).unwrap_or(name);
    match short {
        "main" | "master" | "trunk" | "develop" | "dev" => 0,
        _ if name.contains("release") => 1,
        _ => 3,
    }
}

/// Reads the merged branch's name from a merge commit message, such as
/// `Merge branch 'feat/x'` or `Merge pull request #12 from org/fix/y`.
///
/// # Arguments
/// * `summary` - the merge commit's first line
///
/// # Returns
/// The branch name, when the message names one.
pub fn merged_branch_name(summary: &str) -> Option<String> {
    if let Some(rest) = summary.strip_prefix("Merge pull request #") {
        let from = rest.split_once(" from ")?.1;
        let name = from.split_whitespace().next()?;
        return Some(name.split_once('/').map(|(_, b)| b).unwrap_or(name).to_string());
    }
    let rest = summary.strip_prefix("Merge branch '").or_else(|| summary.strip_prefix("Merge remote-tracking branch '"))?;
    Some(rest.split_once('\'')?.0.to_string())
}

/// Works out which branch every loaded commit belongs to, the way people read a graph: the main
/// branch owns its own line, every other branch owns the commits it added on top, and commits of
/// branches that were merged and deleted take the name from the merge message.
///
/// # Arguments
/// * `commits` - commits in topological order, newest first
///
/// # Returns
/// One branch name per commit, `None` where no name can be found.
pub fn branch_owners(commits: &[CommitInfo]) -> Vec<Option<String>> {
    let index: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let mut owners: Vec<Option<String>> = vec![None; commits.len()];
    let mut tips: Vec<(u8, bool, usize, String)> = Vec::new();
    for (i, c) in commits.iter().enumerate() {
        for r in &c.refs {
            let local = match r.kind {
                RefKind::LocalBranch => true,
                RefKind::RemoteBranch => false,
                _ => continue,
            };
            let rank = if r.is_head { trunk_rank(&r.name).min(2) } else { trunk_rank(&r.name) };
            tips.push((rank, !local, i, r.name.clone()));
        }
    }
    tips.sort();
    let claim = |owners: &mut Vec<Option<String>>, start: usize, name: &str| {
        let mut at = Some(start);
        while let Some(i) = at {
            if owners[i].is_some() {
                break;
            }
            owners[i] = Some(name.to_string());
            at = commits[i].parents.first().and_then(|p| index.get(p.as_str()).copied());
        }
    };
    for (_, _, i, name) in &tips {
        claim(&mut owners, *i, name);
    }
    for c in commits {
        if c.parents.len() < 2 {
            continue;
        }
        let name = merged_branch_name(&c.summary);
        for p in &c.parents[1..] {
            if let (Some(&j), Some(name)) = (index.get(p.as_str()), name.as_ref()) {
                claim(&mut owners, j, name);
            }
        }
    }
    for i in 0..commits.len() {
        let Some(name) = owners[i].clone() else { continue };
        for p in &commits[i].parents {
            if let Some(&j) = index.get(p.as_str())
                && owners[j].is_none()
            {
                owners[j] = Some(name.clone());
            }
        }
    }
    owners
}

/// Gives every branch its own color index, in the order the branches first appear.
///
/// # Arguments
/// * `owners` - from [`branch_owners`]
///
/// # Returns
/// One color index per commit and the index each branch name got.
pub fn owner_colors(owners: &[Option<String>]) -> (Vec<usize>, HashMap<String, usize>) {
    let mut map: HashMap<String, usize> = HashMap::new();
    let colors = owners
        .iter()
        .map(|o| match o {
            Some(name) => {
                let next = map.len();
                *map.entry(name.clone()).or_insert(next)
            }
            None => usize::MAX,
        })
        .collect();
    (colors, map)
}

/// Assigns lanes and line segments to commits for drawing a commit graph.
///
/// # Arguments
/// * `commits` - commits in topological order, newest first
///
/// # Returns
/// One row per commit, in the same order.
pub fn layout(commits: &[CommitInfo]) -> Vec<GraphRow> {
    layout_focused(commits, None, None)
}

/// Assigns lanes and line segments, dimming everything that is not part of a highlighted branch.
///
/// # Arguments
/// * `commits` - commits in topological order, newest first
/// * `hot` - one flag per commit from [`reachable`]; `None` highlights everything
/// * `palette` - one color index per commit from [`owner_colors`], so every branch keeps one
///   color; `None` gives every new lane the next color
///
/// # Returns
/// One row per commit, in the same order.
pub fn layout_focused(commits: &[CommitInfo], hot: Option<&[bool]>, palette: Option<&[usize]>) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut colors: Vec<usize> = Vec::new();
    let mut lane_hot: Vec<bool> = Vec::new();
    let mut next_color = 0usize;
    let mut rows = Vec::with_capacity(commits.len());
    let known: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let is_hot = |i: usize| hot.is_none_or(|h| h.get(i).copied().unwrap_or(false));
    let mut pick_color = |row: Option<usize>| -> usize {
        match row.and_then(|r| palette.and_then(|p| p.get(r).copied())).filter(|c| *c != usize::MAX) {
            Some(c) => c,
            None => {
                next_color += 1;
                next_color - 1
            }
        }
    };

    let alloc = |lanes: &mut Vec<Option<String>>, colors: &mut Vec<usize>, lane_hot: &mut Vec<bool>, id: String, on: bool, color: usize| -> usize {
        if let Some(i) = lanes.iter().position(Option::is_none) {
            lanes[i] = Some(id);
            colors[i] = color;
            lane_hot[i] = on;
            i
        } else {
            lanes.push(Some(id));
            colors.push(color);
            lane_hot.push(on);
            lanes.len() - 1
        }
    };

    for (row_ix, commit) in commits.iter().enumerate() {
        let on = is_hot(row_ix);
        let matched: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_deref() == Some(commit.id.as_str()))
            .map(|(i, _)| i)
            .collect();

        let row_color = palette.map(|_| pick_color(Some(row_ix)));
        let lane = match matched.first() {
            Some(&i) => i,
            None => {
                let c = row_color.unwrap_or_else(|| pick_color(None));
                let i = alloc(&mut lanes, &mut colors, &mut lane_hot, commit.id.clone(), on, c);
                lanes[i] = None;
                i
            }
        };
        let color = row_color.unwrap_or(colors[lane]);
        let mut lines = Vec::new();

        for (i, slot) in lanes.iter().enumerate() {
            if slot.is_some() && !matched.contains(&i) && i != lane {
                lines.push(GraphLine { from: i, to: i, part: LinePart::Through, color: colors[i], dim: !lane_hot[i] });
            }
        }
        for &i in &matched {
            lines.push(GraphLine { from: i, to: lane, part: LinePart::Top, color: colors[i], dim: !lane_hot[i] });
        }

        for &i in &matched {
            lanes[i] = None;
        }

        for (n, parent) in commit.parents.iter().enumerate() {
            if !known.contains_key(parent.as_str()) && n > 0 {
                continue;
            }
            if n == 0
                && let Some(existing) = lanes.iter().position(|l| l.as_deref() == Some(parent.as_str())).filter(|e| *e < lane)
            {
                lane_hot[existing] |= on;
                let c = if palette.is_some() { color } else { colors[existing] };
                lines.push(GraphLine { from: lane, to: existing, part: LinePart::Bottom, color: c, dim: !on });
                continue;
            }
            if n == 0 && lanes[lane].is_none() {
                lanes[lane] = Some(parent.clone());
                lane_hot[lane] = on;
                colors[lane] = color;
                lines.push(GraphLine { from: lane, to: lane, part: LinePart::Bottom, color, dim: !on });
                continue;
            }
            if let Some(existing) = lanes.iter().position(|l| l.as_deref() == Some(parent.as_str())) {
                lane_hot[existing] |= on;
                let c = if palette.is_some() && n == 0 { color } else { colors[existing] };
                lines.push(GraphLine { from: lane, to: existing, part: LinePart::Bottom, color: c, dim: !on });
                continue;
            }
            let c = if n == 0 && palette.is_some() { color } else { pick_color(known.get(parent.as_str()).copied()) };
            let target = alloc(&mut lanes, &mut colors, &mut lane_hot, parent.clone(), on, c);
            lines.push(GraphLine { from: lane, to: target, part: LinePart::Bottom, color: colors[target], dim: !on });
        }

        while matches!(lanes.last(), Some(None)) {
            lanes.pop();
            colors.pop();
            lane_hot.pop();
        }

        let width = lines
            .iter()
            .map(|l| l.from.max(l.to) + 1)
            .max()
            .unwrap_or(0)
            .max(lane + 1);
        rows.push(GraphRow { lane, color, lines, width, dim: !on });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, parents: &[&str]) -> CommitInfo {
        CommitInfo {
            id: id.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            summary: String::new(),
            refs: Vec::new(),
        }
    }

    #[test]
    fn linear_history_stays_in_lane_zero() {
        let rows = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert!(rows.iter().all(|r| r.lane == 0));
        assert_eq!(rows[2].lines.len(), 1);
    }

    #[test]
    fn branch_and_merge_use_second_lane() {
        let rows = layout(&[
            c("m", &["a2", "b1"]),
            c("b1", &["a1"]),
            c("a2", &["a1"]),
            c("a1", &[]),
        ]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[3].lane, 0);
        assert!(rows[3].lines.iter().any(|l| l.part == LinePart::Top && l.from == 1));
    }

    #[test]
    fn commits_off_the_focused_branch_are_dimmed() {
        let commits = [c("side", &["a1"]), c("main", &["a1"]), c("a1", &["a0"]), c("a0", &[])];
        let hot = reachable(&commits, "main").expect("tip is loaded");
        assert_eq!(hot, vec![false, true, true, true]);
        let rows = layout_focused(&commits, Some(&hot), None);
        assert!(rows[0].dim);
        assert!(rows[0].lines.iter().all(|l| l.dim));
        assert!(!rows[1].dim);
        assert!(rows[1].lines.iter().any(|l| l.part == LinePart::Bottom && !l.dim));
        assert!(!rows[2].dim);
        assert!(rows[2].lines.iter().any(|l| l.part == LinePart::Top && !l.dim));
        assert!(layout(&commits).iter().all(|r| !r.dim && r.lines.iter().all(|l| !l.dim)));
        assert!(reachable(&commits, "missing").is_none());
    }

    fn tip(id: &str, parents: &[&str], branch: &str, head: bool) -> CommitInfo {
        let mut commit = c(id, parents);
        commit.refs.push(crate::log::RefLabel { name: branch.into(), kind: RefKind::LocalBranch, is_head: head });
        commit
    }

    #[test]
    fn reads_merged_branch_names() {
        assert_eq!(merged_branch_name("Merge branch 'feat/units'").as_deref(), Some("feat/units"));
        assert_eq!(merged_branch_name("Merge branch 'feat/x' into main").as_deref(), Some("feat/x"));
        assert_eq!(merged_branch_name("Merge pull request #86 from Second-Choice/fix/scrp-cards-joiners").as_deref(), Some("fix/scrp-cards-joiners"));
        assert_eq!(merged_branch_name("Merge remote-tracking branch 'origin/main' into feat/b").as_deref(), Some("origin/main"));
        assert_eq!(merged_branch_name("fix: something"), None);
    }

    #[test]
    fn every_commit_gets_the_branch_it_was_made_on() {
        let commits = [
            tip("f2", &["f1"], "feat/search", true),
            tip("m3", &["m2", "x1"], "main", false),
            c("x1", &["m1"]),
            c("f1", &["m2"]),
            c("m2", &["m1"]),
            c("m1", &[]),
        ];
        let mut merge = commits.clone();
        merge[1].summary = "Merge branch 'fix/cache'".into();
        let mut checked_out_main = merge.clone();
        checked_out_main[1].refs = vec![crate::log::RefLabel { name: "main".into(), kind: RefKind::LocalBranch, is_head: true }, crate::log::RefLabel { name: "origin/main".into(), kind: RefKind::RemoteBranch, is_head: false }];
        checked_out_main[0].refs[0].is_head = false;
        assert_eq!(branch_owners(&checked_out_main)[4].as_deref(), Some("main"), "the local branch wins over its remote copy");
        let owners = branch_owners(&merge);
        assert_eq!(owners[0].as_deref(), Some("feat/search"));
        assert_eq!(owners[1].as_deref(), Some("main"));
        assert_eq!(owners[2].as_deref(), Some("fix/cache"));
        assert_eq!(owners[3].as_deref(), Some("feat/search"));
        assert_eq!(owners[4].as_deref(), Some("main"));
        assert_eq!(owners[5].as_deref(), Some("main"));
        let (palette, map) = owner_colors(&owners);
        assert_eq!(palette[0], map["feat/search"]);
        assert_eq!(palette[3], palette[0]);
        assert_ne!(palette[1], palette[0]);
        let rows = layout_focused(&merge, None, Some(&palette));
        assert_eq!(rows[3].color, palette[0]);
        assert_eq!(rows[4].color, palette[1]);
    }

    #[test]
    fn many_branches_off_one_commit_join_the_existing_lane() {
        let mut commits: Vec<CommitInfo> = (0..18).map(|i| c(&format!("tip{i}"), &["root"])).collect();
        commits.push(c("root", &[]));
        let rows = layout(&commits);
        assert!(rows.iter().all(|r| r.width <= 2), "graph stays two lanes wide");
        assert_eq!(rows[0].lane, 0);
        assert!(rows[1..18].iter().all(|r| r.lane == 1));
        assert!(rows[5].lines.iter().any(|l| l.part == LinePart::Bottom && l.from == 1 && l.to == 0));
        assert_eq!(rows[18].lane, 0);
    }
}
