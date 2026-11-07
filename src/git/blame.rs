use super::exec::{GitError, run_git_text};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct BlameShare {
    pub author_email: String,
    pub lines: usize,
}

/// Runs `git blame --line-porcelain` on one file and tallies line counts per
/// author email. Callers are expected to skip files over
/// `[git] blame_max_file_lines` before calling this, to bound cost.
pub fn blame_file(root: &Path, relative_path: &str) -> Result<Vec<BlameShare>, GitError> {
    let text = run_git_text(root, &["blame", "--line-porcelain", "--", relative_path])?;
    let mut counts = BTreeMap::<String, usize>::new();

    for line in text.lines() {
        if let Some(email) = line.strip_prefix("author-mail ") {
            let email = email.trim().trim_start_matches('<').trim_end_matches('>');
            *counts.entry(email.to_string()).or_insert(0) += 1;
        }
    }

    let mut shares = counts
        .into_iter()
        .map(|(author_email, lines)| BlameShare {
            author_email,
            lines,
        })
        .collect::<Vec<_>>();
    shares.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then_with(|| left.author_email.cmp(&right.author_email))
    });

    Ok(shares)
}

#[cfg(test)]
mod tests {
    use super::run_git_text;
    use std::path::Path;

    #[test]
    fn run_git_text_reports_not_a_repository_outside_a_repo() {
        let root =
            std::env::temp_dir().join(format!("gdhealth-blame-not-a-repo-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temp dir");

        let result = run_git_text(&root, &["blame", "--line-porcelain", "--", "missing.gd"]);
        assert!(result.is_err());

        std::fs::remove_dir_all(&root).ok();
        let _ = Path::new(&root);
    }
}
