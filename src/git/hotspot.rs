use super::log_walk::CommitRecord;
use crate::config::GitConfig;
use crate::model::{CoChangePair, GitOwnershipShare};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

const SECONDS_PER_DAY: f64 = 86_400.0;
const MIN_RECENT_COMMITS_FOR_HOTSPOT: usize = 3;
const HOTSPOT_TOP_QUARTILE: f64 = 0.75;

// Commit-subject classifier for "this commit fixed a bug". INCLUDE must
// match and EXCLUDE must not, so routine chore/format/dependency-bump
// commits that happen to mention "fix" in passing don't count.
static FIX_SIGNAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(fix|repair|resolve[ds]?|correct(?:ion|ed)?|hotfix|regression|defect|crash)\b|\bfix(?:es|ed)?\s+(?:#\d+|issue)")
        .expect("valid fix-include regex")
});
static NON_DEFECT_SIGNAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(merge|documentation|readme|spelling|format(?:ting)?|lint(?:ing)?|dependency|dependencies|version bump|release)\b")
        .expect("valid fix-exclude regex")
});

fn is_fix_commit(subject: &str) -> bool {
    FIX_SIGNAL_RE.is_match(subject) && !NON_DEFECT_SIGNAL_RE.is_match(subject)
}

#[derive(Debug, Clone, Default)]
pub struct MutableFileStats {
    pub commit_count_total: usize,
    pub commit_count_90d: usize,
    pub commit_count_30d: usize,
    pub lines_added_90d: usize,
    pub lines_deleted_90d: usize,
    pub temporal_hotspot_score: f64,
    pub change_entropy: f64,
    pub prior_defect_count: usize,
    pub ownership: Vec<GitOwnershipShare>,
    pub churn_percentile: f64,
    pub is_churn_hotspot: bool,
}

/// Accumulates per-file git signals from a single pass over `commits`.
/// `anchor_time` is the repo's own last-commit timestamp (not wall clock),
/// so results stay reproducible regardless of when the command runs.
pub fn compute_file_stats(
    commits: &[CommitRecord],
    known_files: &BTreeSet<String>,
    config: &GitConfig,
    anchor_time: i64,
) -> BTreeMap<String, MutableFileStats> {
    let mut stats = BTreeMap::<String, MutableFileStats>::new();
    let halflife = config.hotspot_halflife_days.max(0.001);
    let entropy_tau = config.co_change_window_days.max(0.001);
    let ln2 = std::f64::consts::LN_2;

    for commit in commits {
        let touched_count = commit.files.len();
        if touched_count == 0 {
            continue;
        }

        let age_days = ((anchor_time - commit.unix_time) as f64 / SECONDS_PER_DAY).max(0.0);
        let entropy_weight = (-age_days / entropy_tau).exp();
        let entropy_share = (touched_count as f64).log2() / touched_count as f64;
        let is_fix = is_fix_commit(&commit.subject);
        let within_defect_window = age_days <= config.prior_defect_window_days as f64;
        let decay = (-ln2 * age_days / halflife).exp();

        for delta in &commit.files {
            if !known_files.contains(&delta.path) {
                continue;
            }

            let entry = stats.entry(delta.path.clone()).or_default();
            entry.commit_count_total += 1;
            if age_days <= 90.0 {
                entry.commit_count_90d += 1;
                entry.lines_added_90d += delta.added;
                entry.lines_deleted_90d += delta.deleted;
            }
            if age_days <= 30.0 {
                entry.commit_count_30d += 1;
            }

            let churn = ((delta.added + delta.deleted) as f64 / 100.0).min(3.0);
            entry.temporal_hotspot_score += decay * churn;
            entry.change_entropy += entropy_weight * entropy_share;
            if is_fix && within_defect_window {
                entry.prior_defect_count += 1;
            }
        }
    }

    stats
}

/// Repo-wide bus factor: the smallest set of contributors whose combined
/// commit share reaches 80% of all commits.
pub fn compute_bus_factor(commits: &[CommitRecord]) -> (usize, Vec<String>) {
    let mut counts = BTreeMap::<String, usize>::new();
    for commit in commits {
        *counts.entry(commit.author_email.clone()).or_insert(0) += 1;
    }

    let total: usize = counts.values().sum();
    if total == 0 {
        return (0, Vec::new());
    }

    let mut sorted = counts.into_iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    let threshold = total as f64 * 0.8;
    let mut cumulative = 0usize;
    let mut contributors = Vec::new();
    for (author, count) in sorted {
        cumulative += count;
        contributors.push(author);
        if cumulative as f64 >= threshold {
            break;
        }
    }

    let bus_factor = contributors.len();
    (bus_factor, contributors)
}

/// Co-change pairs from a single `git log --name-only`-equivalent walk.
/// Commits touching more than `co_change_max_files_per_commit` files
/// (measured across the whole repo, not just tracked GDScript files) are
/// excluded as mass-edit noise.
pub fn compute_co_change_pairs(
    commits: &[CommitRecord],
    known_files: &BTreeSet<String>,
    config: &GitConfig,
    anchor_time: i64,
) -> Vec<CoChangePair> {
    let mut weights = BTreeMap::<(String, String), f64>::new();
    let tau = config.co_change_window_days.max(0.001);

    for commit in commits {
        let raw_touched_count = commit.files.len();
        if raw_touched_count < 2 || raw_touched_count > config.co_change_max_files_per_commit {
            continue;
        }

        let mut paths = commit
            .files
            .iter()
            .map(|delta| delta.path.clone())
            .filter(|path| known_files.contains(path))
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        if paths.len() < 2 {
            continue;
        }

        let age_days = ((anchor_time - commit.unix_time) as f64 / SECONDS_PER_DAY).max(0.0);
        let weight = (-age_days / tau).exp();

        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                let key = (paths[i].clone(), paths[j].clone());
                *weights.entry(key).or_insert(0.0) += weight;
            }
        }
    }

    let mut pairs = weights
        .into_iter()
        .map(|((file_a, file_b), weight)| CoChangePair {
            file_a,
            file_b,
            weight,
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| {
        right
            .weight
            .partial_cmp(&left.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.file_a.cmp(&right.file_a))
            .then_with(|| left.file_b.cmp(&right.file_b))
    });

    pairs
}

/// Ranks files by temporal hotspot score and flags the top quartile as
/// churn hotspots, gated by an absolute activity floor so a quiet repo's
/// top quartile doesn't degenerate into "anything touched recently".
pub fn apply_percentiles(stats: &mut BTreeMap<String, MutableFileStats>) {
    let mut ranked = stats
        .iter()
        .map(|(path, s)| (path.clone(), s.temporal_hotspot_score, s.commit_count_90d))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.cmp(&right.0))
    });

    let total = ranked.len();
    for (rank, (path, _, commit_count_90d)) in ranked.into_iter().enumerate() {
        let percentile = if total <= 1 {
            1.0
        } else {
            1.0 - (rank as f64 / (total - 1) as f64)
        };
        let is_hotspot = percentile >= HOTSPOT_TOP_QUARTILE
            && commit_count_90d >= MIN_RECENT_COMMITS_FOR_HOTSPOT;

        if let Some(entry) = stats.get_mut(&path) {
            entry.churn_percentile = percentile;
            entry.is_churn_hotspot = is_hotspot;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::log_walk::FileDelta;

    fn commit(
        hash: &str,
        days_ago: i64,
        author: &str,
        subject: &str,
        files: &[(&str, usize, usize)],
    ) -> CommitRecord {
        CommitRecord {
            hash: hash.to_string(),
            unix_time: 1_700_000_000 - days_ago * 86_400,
            author_email: author.to_string(),
            subject: subject.to_string(),
            files: files
                .iter()
                .map(|(path, added, deleted)| FileDelta {
                    path: path.to_string(),
                    added: *added,
                    deleted: *deleted,
                })
                .collect(),
        }
    }

    #[test]
    fn bus_factor_covers_80_percent_of_commits() {
        let commits = vec![
            commit("a", 0, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("b", 1, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("c", 2, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("d", 3, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("e", 4, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("f", 5, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("g", 6, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("h", 7, "alice@example.com", "work", &[("f.gd", 1, 0)]),
            commit("i", 8, "bob@example.com", "work", &[("f.gd", 1, 0)]),
            commit("j", 9, "carol@example.com", "work", &[("f.gd", 1, 0)]),
        ];
        // alice has 8/10 = 80% already, so she alone reaches the threshold.
        let (bus_factor, contributors) = compute_bus_factor(&commits);
        assert_eq!(bus_factor, 1);
        assert_eq!(contributors, vec!["alice@example.com".to_string()]);
    }

    #[test]
    fn prior_defect_count_matches_fix_commits_within_window_only() {
        let known = BTreeSet::from(["f.gd".to_string()]);
        let config = GitConfig::default();
        let commits = vec![
            commit(
                "a",
                10,
                "dev@example.com",
                "fix: crash on load",
                &[("f.gd", 2, 1)],
            ),
            commit(
                "b",
                10,
                "dev@example.com",
                "chore: bump deps",
                &[("f.gd", 1, 1)],
            ),
            commit(
                "c",
                400,
                "dev@example.com",
                "fix: old bug",
                &[("f.gd", 3, 0)],
            ),
        ];
        let stats = compute_file_stats(&commits, &known, &config, 1_700_000_000);
        let entry = &stats["f.gd"];
        // Only commit "a" is both a fix AND within the 180-day window.
        assert_eq!(entry.prior_defect_count, 1);
        assert_eq!(entry.commit_count_total, 3);
    }

    #[test]
    fn co_change_pairs_ignore_mass_edit_commits() {
        let known = BTreeSet::from(["a.gd".to_string(), "b.gd".to_string()]);
        let config = GitConfig {
            co_change_max_files_per_commit: 2,
            ..Default::default()
        };
        let commits = vec![
            commit(
                "x",
                0,
                "dev@example.com",
                "work",
                &[("a.gd", 1, 0), ("b.gd", 1, 0)],
            ),
            commit(
                "y",
                1,
                "dev@example.com",
                "mass rename",
                &[("a.gd", 1, 0), ("b.gd", 1, 0), ("c.gd", 1, 0)],
            ),
        ];
        let pairs = compute_co_change_pairs(&commits, &known, &config, 1_700_000_000);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].file_a, "a.gd");
        assert_eq!(pairs[0].file_b, "b.gd");
    }

    #[test]
    fn percentiles_flag_only_top_quartile_with_activity_floor() {
        let mut stats = BTreeMap::new();
        for i in 0..8 {
            let entry = MutableFileStats {
                temporal_hotspot_score: (8 - i) as f64,
                commit_count_90d: 5,
                ..Default::default()
            };
            stats.insert(format!("file{i}.gd"), entry);
        }
        apply_percentiles(&mut stats);
        assert!(stats["file0.gd"].is_churn_hotspot);
        assert!(!stats["file7.gd"].is_churn_hotspot);
    }
}
