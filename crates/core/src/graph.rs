use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::log::CommitInfo;

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
}

/// Graph drawing data for one commit row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    pub lines: Vec<GraphLine>,
    pub width: usize,
}

/// Assigns lanes and line segments to commits for drawing a commit graph.
///
/// # Arguments
/// * `commits` - commits in topological order, newest first
///
/// # Returns
/// One row per commit, in the same order.
pub fn layout(commits: &[CommitInfo]) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut colors: Vec<usize> = Vec::new();
    let mut next_color = 0usize;
    let mut rows = Vec::with_capacity(commits.len());
    let known: HashMap<&str, ()> = commits.iter().map(|c| (c.id.as_str(), ())).collect();

    let mut alloc = |lanes: &mut Vec<Option<String>>, colors: &mut Vec<usize>, id: String| -> usize {
        let color = next_color;
        next_color += 1;
        if let Some(i) = lanes.iter().position(Option::is_none) {
            lanes[i] = Some(id);
            colors[i] = color;
            i
        } else {
            lanes.push(Some(id));
            colors.push(color);
            lanes.len() - 1
        }
    };

    for commit in commits {
        let matched: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_deref() == Some(commit.id.as_str()))
            .map(|(i, _)| i)
            .collect();

        let lane = match matched.first() {
            Some(&i) => i,
            None => {
                let i = alloc(&mut lanes, &mut colors, commit.id.clone());
                lanes[i] = None;
                i
            }
        };
        let color = colors[lane];
        let mut lines = Vec::new();

        for (i, slot) in lanes.iter().enumerate() {
            if slot.is_some() && !matched.contains(&i) && i != lane {
                lines.push(GraphLine { from: i, to: i, part: LinePart::Through, color: colors[i] });
            }
        }
        for &i in &matched {
            lines.push(GraphLine { from: i, to: lane, part: LinePart::Top, color: colors[i] });
        }

        for &i in &matched {
            lanes[i] = None;
        }

        for (n, parent) in commit.parents.iter().enumerate() {
            if !known.contains_key(parent.as_str()) && n > 0 {
                continue;
            }
            if n == 0 && lanes[lane].is_none() {
                lanes[lane] = Some(parent.clone());
                lines.push(GraphLine { from: lane, to: lane, part: LinePart::Bottom, color });
                continue;
            }
            if let Some(existing) = lanes.iter().position(|l| l.as_deref() == Some(parent.as_str())) {
                lines.push(GraphLine { from: lane, to: existing, part: LinePart::Bottom, color: colors[existing] });
                continue;
            }
            let target = alloc(&mut lanes, &mut colors, parent.clone());
            lines.push(GraphLine { from: lane, to: target, part: LinePart::Bottom, color: colors[target] });
        }

        while matches!(lanes.last(), Some(None)) {
            lanes.pop();
            colors.pop();
        }

        let width = lines
            .iter()
            .map(|l| l.from.max(l.to) + 1)
            .max()
            .unwrap_or(0)
            .max(lane + 1);
        rows.push(GraphRow { lane, color, lines, width });
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
}
