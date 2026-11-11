use std::cmp::Ordering;

/// One piece of a release's notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(String),
    Para(String),
    List(Vec<String>),
}

/// One `## [version] - date` section of CHANGELOG.md.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub date: String,
    pub blocks: Vec<Block>,
}

/// Compares two dotted version strings numerically, so `0.10.0` sorts after `0.9.0`.
///
/// # Arguments
/// * `a` - first version, such as `1.2.0`
/// * `b` - second version
///
/// # Returns
/// How `a` orders against `b`. Non-numeric parts such as `Unreleased` count as zero.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split(['.', '-']).map(|p| p.parse().unwrap_or(0)).collect() };
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let ord = pa.get(i).copied().unwrap_or(0).cmp(&pb.get(i).copied().unwrap_or(0));
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

/// Parses a Keep a Changelog style file into releases, newest first.
///
/// # Arguments
/// * `text` - the CHANGELOG.md contents
///
/// # Returns
/// Every versioned release section. The `[Unreleased]` section is skipped.
pub fn parse(text: &str) -> Vec<Release> {
    let mut releases: Vec<Release> = Vec::new();
    let mut current: Option<Release> = None;
    let mut item: Option<String> = None;
    let flush_item = |item: &mut Option<String>, release: &mut Option<Release>| {
        if let (Some(text), Some(r)) = (item.take(), release.as_mut()) {
            match r.blocks.last_mut() {
                Some(Block::List(items)) => items.push(text),
                _ => r.blocks.push(Block::List(vec![text])),
            }
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## [") {
            flush_item(&mut item, &mut current);
            if let Some(r) = current.take() {
                releases.push(r);
            }
            let Some((version, tail)) = rest.split_once(']') else { continue };
            if version.eq_ignore_ascii_case("unreleased") {
                continue;
            }
            let date = tail.trim().trim_start_matches(['-', '–']).trim().to_string();
            current = Some(Release { version: version.trim().to_string(), date, blocks: Vec::new() });
            continue;
        }
        if current.is_none() {
            continue;
        }
        if trimmed.is_empty() {
            flush_item(&mut item, &mut current);
            continue;
        }
        if let Some(h) = trimmed.strip_prefix("### ") {
            flush_item(&mut item, &mut current);
            if let Some(r) = current.as_mut() {
                r.blocks.push(Block::Heading(h.trim().to_string()));
            }
        } else if let Some(text) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            flush_item(&mut item, &mut current);
            item = Some(text.trim().to_string());
        } else if let Some(open) = item.as_mut() {
            open.push(' ');
            open.push_str(trimmed);
        } else if let Some(r) = current.as_mut() {
            r.blocks.push(Block::Para(trimmed.to_string()));
        }
    }
    flush_item(&mut item, &mut current);
    if let Some(r) = current {
        releases.push(r);
    }
    releases.sort_by(|a, b| compare_versions(&b.version, &a.version));
    releases
}

/// Picks the releases a user has not seen yet.
///
/// # Arguments
/// * `releases` - parsed releases, newest first
/// * `since` - the version the user ran before the update; empty for "show recent history"
/// * `current` - the version now running
/// * `limit` - the most releases to return when `since` is empty
///
/// # Returns
/// Releases newer than `since` up to and including `current`, newest first.
pub fn between(releases: &[Release], since: &str, current: &str, limit: usize) -> Vec<Release> {
    let picked = releases
        .iter()
        .filter(|r| compare_versions(&r.version, current) != Ordering::Greater)
        .filter(|r| since.is_empty() || compare_versions(&r.version, since) == Ordering::Greater)
        .cloned();
    if since.is_empty() { picked.take(limit).collect() } else { picked.collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Changelog\n\nIntro text.\n\n## [Unreleased]\n\n- Not shipped yet\n\n## [0.10.0] - 2026-12-01\n\n### Added\n\n- Big thing that wraps\n  onto a second line\n- Small thing\n\nA closing note.\n\n## [0.9.0] - 2026-11-20\n\n### Fixed\n\n* A crash\n\n## [0.2.0] – 2026-11-12\n\n- First update\n";

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare_versions("0.10.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("v1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.2.3", "1.2.4"), Ordering::Less);
    }

    #[test]
    fn parses_sections_headings_and_wrapped_items() {
        let releases = parse(SAMPLE);
        assert_eq!(releases.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(), ["0.10.0", "0.9.0", "0.2.0"]);
        let first = &releases[0];
        assert_eq!(first.date, "2026-12-01");
        assert_eq!(
            first.blocks,
            vec![
                Block::Heading("Added".into()),
                Block::List(vec!["Big thing that wraps onto a second line".into(), "Small thing".into()]),
                Block::Para("A closing note.".into()),
            ]
        );
        assert_eq!(releases[1].blocks[1], Block::List(vec!["A crash".into()]));
        assert_eq!(releases[2].date, "2026-11-12");
    }

    #[test]
    fn picks_only_unseen_releases() {
        let releases = parse(SAMPLE);
        let seen = between(&releases, "0.2.0", "0.10.0", 5);
        assert_eq!(seen.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(), ["0.10.0", "0.9.0"]);
        assert!(between(&releases, "0.10.0", "0.10.0", 5).is_empty());
        assert_eq!(between(&releases, "", "0.9.0", 1).len(), 1);
        assert_eq!(between(&releases, "", "0.9.0", 1)[0].version, "0.9.0");
    }
}
