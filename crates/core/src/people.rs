use crate::git::Git;
use crate::Result;

/// Someone who has made commits in the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub email: String,
    pub commits: u32,
}

impl Person {
    /// The person as a `Co-authored-by` trailer.
    ///
    /// # Returns
    /// A line such as `Co-authored-by: Ada <ada@example.com>`.
    pub fn trailer(&self) -> String {
        format!("{TRAILER} {} <{}>", self.name, self.email)
    }
}

const TRAILER: &str = "Co-authored-by:";

fn parse_shortlog(out: &str) -> Vec<Person> {
    out.lines()
        .filter_map(|line| {
            let (count, rest) = line.trim().split_once('\t')?;
            let (name, email) = rest.rsplit_once(" <")?;
            Some(Person { name: name.trim().to_string(), email: email.trim_end_matches('>').to_string(), commits: count.trim().parse().ok()? })
        })
        .collect()
}

/// Lists everyone who made commits on any branch, most active first, without the current user.
///
/// # Arguments
/// * `git` - runner for the repository
///
/// # Returns
/// Up to 200 people, each email listed once.
pub fn contributors(git: &Git) -> Result<Vec<Person>> {
    let me = git.run(["config", "user.email"]).map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let out = git.run(["shortlog", "-sne", "--all", "--no-merges"])?;
    let mut seen = std::collections::HashSet::new();
    let mut people: Vec<Person> = parse_shortlog(&out)
        .into_iter()
        .filter(|p| !p.email.is_empty() && p.email.to_lowercase() != me && !p.email.contains("[bot]") && !p.name.ends_with("[bot]"))
        .filter(|p| seen.insert(p.email.to_lowercase()))
        .collect();
    people.sort_by_key(|p| std::cmp::Reverse(p.commits));
    people.truncate(200);
    Ok(people)
}

/// The co-author emails already named in a commit message.
///
/// # Arguments
/// * `body` - the commit description
///
/// # Returns
/// Lowercased email addresses.
pub fn co_author_emails(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| l.trim().strip_prefix(TRAILER))
        .filter_map(|rest| rest.rsplit_once('<').map(|(_, e)| e.trim_end_matches('>').trim().to_lowercase()))
        .collect()
}

/// Replaces the co-author trailers at the end of a commit description.
///
/// # Arguments
/// * `body` - the current description
/// * `people` - the co-authors to list; none removes the trailers
///
/// # Returns
/// The description with one `Co-authored-by` line per person after a blank line.
pub fn set_co_authors(body: &str, people: &[Person]) -> String {
    let kept: Vec<&str> = body.lines().filter(|l| !l.trim().starts_with(TRAILER)).collect();
    let text = kept.join("\n").trim_end().to_string();
    if people.is_empty() {
        return text;
    }
    let trailers = people.iter().map(Person::trailer).collect::<Vec<_>>().join("\n");
    if text.is_empty() { trailers } else { format!("{text}\n\n{trailers}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortlog_lines_become_people() {
        let people = parse_shortlog("    12\tAda Lovelace <ada@example.com>\n     3\tLinus <l@x.org>\n");
        assert_eq!(people[0], Person { name: "Ada Lovelace".into(), email: "ada@example.com".into(), commits: 12 });
        assert_eq!(people[1].trailer(), "Co-authored-by: Linus <l@x.org>");
    }

    #[test]
    fn co_author_trailers_are_replaced_at_the_end() {
        let ada = Person { name: "Ada".into(), email: "ada@x.org".into(), commits: 1 };
        let body = set_co_authors("Fix the parser.", std::slice::from_ref(&ada));
        assert_eq!(body, "Fix the parser.\n\nCo-authored-by: Ada <ada@x.org>");
        assert_eq!(co_author_emails(&body), ["ada@x.org"]);
        assert_eq!(set_co_authors(&body, &[]), "Fix the parser.");
        assert_eq!(set_co_authors("", std::slice::from_ref(&ada)), "Co-authored-by: Ada <ada@x.org>");
    }
}
