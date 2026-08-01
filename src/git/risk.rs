//! Self-relative change-risk scoring for a commit or `base..head` range.
//!
//! The score compares a change's diff shape with this repository's recent
//! history. It is a transparent heuristic, not a defect-prediction model.

use super::exec::{GitError, run_git_text};
use super::log_walk::{CommitRecord, walk_commits};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

const REFERENCE_SAMPLE_SIZE: usize = 150;

#[derive(Debug, Clone)]
pub struct CommitChangeShape {
    pub lines_added: f64,
    pub lines_deleted: f64,
    pub files_touched: f64,
    pub directories_touched: f64,
    pub subsystems_touched: f64,
    pub entropy: f64,
    pub author_experience: Option<f64>,
    pub ref_label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskFactor {
    pub feature: String,
    pub value: Option<f64>,
    pub percentile: Option<f64>,
    pub contribution: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitRiskScore {
    pub score: f64,
    pub level: String,
    pub factors: Vec<RiskFactor>,
    pub ref_label: String,
    pub reference_commit_count: usize,
    pub method_note: &'static str,
}

/// Scores a commit (no `..` in `revspec`) or a `base..head` range.
pub fn score_revspec(root: &Path, revspec: &str) -> Result<CommitRiskScore, String> {
    let target = if let Some((base, head)) = revspec.split_once("..") {
        extract_range_shape(root, base, head)
    } else {
        extract_commit_shape(root, revspec)
    }
    .map_err(|error| error.to_string())?;

    let reference = reference_shapes(root).map_err(|error| error.to_string())?;
    Ok(score_commit(&target, &reference))
}

fn extract_commit_shape(root: &Path, sha: &str) -> Result<CommitChangeShape, GitError> {
    let meta = run_git_text(root, &["show", "-s", "--format=%ae", sha])?;
    let author = meta.trim();
    let numstat = run_git_text(root, &["show", sha, "--numstat", "--format="])?;
    let mut shape = shape_from_numstat(&numstat, sha.to_string());
    let parent = run_git_text(
        root,
        &["rev-parse", "--verify", "--quiet", &format!("{sha}^")],
    )
    .unwrap_or_default();
    shape.author_experience = author_experience(root, author, parent.trim());
    Ok(shape)
}

fn extract_range_shape(root: &Path, base: &str, head: &str) -> Result<CommitChangeShape, GitError> {
    let revspec = format!("{base}..{head}");
    let numstat = run_git_text(root, &["diff", "--numstat", &revspec])?;
    let meta = run_git_text(root, &["show", "-s", "--format=%ae", head])?;
    let mut shape = shape_from_numstat(&numstat, revspec);
    shape.author_experience = author_experience(root, meta.trim(), base);
    Ok(shape)
}

fn reference_shapes(root: &Path) -> Result<Vec<CommitChangeShape>, GitError> {
    let mut commits = walk_commits(root)?;
    commits.reverse(); // oldest first, to derive experience available at each commit.
    let mut experience_by_author = HashMap::<String, usize>::new();
    let mut shapes = Vec::new();
    for commit in commits {
        let experience = experience_by_author
            .get(&commit.author_email)
            .copied()
            .unwrap_or(0);
        let mut shape = shape_from_record(&commit);
        shape.author_experience = Some(experience as f64);
        *experience_by_author.entry(commit.author_email).or_default() += 1;
        shapes.push(shape);
    }
    shapes.reverse();
    shapes.truncate(REFERENCE_SAMPLE_SIZE);
    Ok(shapes)
}

fn shape_from_record(commit: &CommitRecord) -> CommitChangeShape {
    let mut numstat = String::new();
    for delta in &commit.files {
        numstat.push_str(&format!(
            "{}\t{}\t{}\n",
            delta.added, delta.deleted, delta.path
        ));
    }
    shape_from_numstat(&numstat, commit.hash.clone())
}

fn shape_from_numstat(numstat: &str, ref_label: String) -> CommitChangeShape {
    let (lines_added, lines_deleted, files_touched, dirs, subs, per_file) =
        accumulate_numstat(numstat);
    CommitChangeShape {
        lines_added,
        lines_deleted,
        files_touched,
        directories_touched: dirs.len() as f64,
        subsystems_touched: subs.len() as f64,
        entropy: shannon_entropy(&per_file),
        author_experience: None,
        ref_label,
    }
}

fn accumulate_numstat(
    numstat: &str,
) -> (f64, f64, f64, BTreeSet<String>, BTreeSet<String>, Vec<f64>) {
    let mut lines_added = 0.0;
    let mut lines_deleted = 0.0;
    let mut files_touched = 0.0;
    let mut dirs = BTreeSet::new();
    let mut subs = BTreeSet::new();
    let mut per_file = Vec::new();
    for row in numstat.lines().map(str::trim).filter(|row| !row.is_empty()) {
        let mut parts = row.splitn(3, '\t');
        let added: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
        let deleted: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
        let path = parts.next().unwrap_or("").trim();
        if path.is_empty() {
            continue;
        }
        lines_added += added;
        lines_deleted += deleted;
        files_touched += 1.0;
        if added + deleted > 0.0 {
            per_file.push(added + deleted);
        }
        let segments = path.split('/').collect::<Vec<_>>();
        dirs.insert(segments[..segments.len().saturating_sub(1)].join("/"));
        subs.insert(segments[0].to_string());
    }
    (
        lines_added,
        lines_deleted,
        files_touched,
        dirs,
        subs,
        per_file,
    )
}

fn shannon_entropy(per_file: &[f64]) -> f64 {
    let total: f64 = per_file.iter().sum();
    if total <= 0.0 || per_file.len() < 2 {
        return 0.0;
    }
    -per_file
        .iter()
        .filter(|&&v| v > 0.0)
        .map(|&v| {
            let p = v / total;
            p * p.log2()
        })
        .sum::<f64>()
}

fn author_experience(root: &Path, author: &str, upto_ref: &str) -> Option<f64> {
    if author.is_empty() || upto_ref.is_empty() {
        return Some(0.0);
    }
    run_git_text(
        root,
        &[
            "rev-list",
            "--count",
            "--author",
            author,
            "--no-merges",
            upto_ref,
        ],
    )
    .ok()?
    .trim()
    .parse()
    .ok()
}

fn score_commit(target: &CommitChangeShape, reference: &[CommitChangeShape]) -> CommitRiskScore {
    let features: [(&str, Option<f64>, f64, bool); 7] = [
        ("lines_added", Some(target.lines_added), 0.24, false),
        ("lines_deleted", Some(target.lines_deleted), 0.14, false),
        ("files_touched", Some(target.files_touched), 0.20, false),
        (
            "directories_touched",
            Some(target.directories_touched),
            0.12,
            false,
        ),
        (
            "subsystems_touched",
            Some(target.subsystems_touched),
            0.10,
            false,
        ),
        ("entropy", Some(target.entropy), 0.10, false),
        ("author_experience", target.author_experience, 0.10, true),
    ];
    let mut total_weight = 0.0;
    let mut weighted = 0.0;
    let mut factors = Vec::new();
    for (name, value, weight, invert) in features {
        let distribution = reference
            .iter()
            .filter_map(|shape| value_for(shape, name))
            .collect::<Vec<_>>();
        let percentile = value.map(|value| percentile_rank(value, &distribution));
        let adjusted = percentile.map(|p| if invert { 1.0 - p } else { p });
        let contribution = adjusted.unwrap_or(0.0) * weight;
        if adjusted.is_some() {
            total_weight += weight;
            weighted += contribution;
        }
        factors.push(RiskFactor {
            feature: name.to_string(),
            value,
            percentile,
            contribution,
            label: factor_label(name, percentile, invert),
        });
    }
    let score = if total_weight == 0.0 {
        0.0
    } else {
        (10.0 * weighted / total_weight * 10.0).round() / 10.0
    };
    CommitRiskScore {
        score,
        level: level_for(score),
        factors,
        ref_label: target.ref_label.clone(),
        reference_commit_count: reference.len(),
        method_note: "heuristic score relative to this repository's recent commit history; not a statistically validated defect predictor",
    }
}

fn value_for(shape: &CommitChangeShape, name: &str) -> Option<f64> {
    Some(match name {
        "lines_added" => shape.lines_added,
        "lines_deleted" => shape.lines_deleted,
        "files_touched" => shape.files_touched,
        "directories_touched" => shape.directories_touched,
        "subsystems_touched" => shape.subsystems_touched,
        "entropy" => shape.entropy,
        "author_experience" => return shape.author_experience,
        _ => return None,
    })
}

fn percentile_rank(value: f64, distribution: &[f64]) -> f64 {
    if distribution.len() <= 1 {
        return 0.5;
    }
    let below_or_equal = distribution
        .iter()
        .filter(|&&sample| sample <= value)
        .count();
    (below_or_equal.saturating_sub(1) as f64 / (distribution.len() - 1) as f64).clamp(0.0, 1.0)
}

fn factor_label(name: &str, percentile: Option<f64>, invert: bool) -> String {
    let Some(percentile) = percentile else {
        return format!("{name} unavailable");
    };
    let risk_percentile = if invert { 1.0 - percentile } else { percentile };
    format!(
        "{:.0}th percentile in recent history",
        risk_percentile * 100.0
    )
}

fn level_for(score: f64) -> String {
    if score >= 7.0 {
        "high"
    } else if score >= 4.0 {
        "moderate"
    } else {
        "low"
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{CommitChangeShape, accumulate_numstat, score_commit, shannon_entropy};
    fn shape(size: f64, files: f64, exp: Option<f64>) -> CommitChangeShape {
        CommitChangeShape {
            lines_added: size,
            lines_deleted: size / 2.0,
            files_touched: files,
            directories_touched: files,
            subsystems_touched: files.min(3.0),
            entropy: files.log2(),
            author_experience: exp,
            ref_label: "x".into(),
        }
    }
    #[test]
    fn accumulates_numstat_into_size_diffusion_and_per_file_churn() {
        let (a, d, f, dirs, subs, per) = accumulate_numstat(
            "10\t2\tgameplay/core/foo.gd\n5\t0\tgameplay/combat/bar.gd\n0\t3\tnet/baz.gd\n",
        );
        assert_eq!((a, d, f), (15.0, 5.0, 3.0));
        assert_eq!(dirs.len(), 3);
        assert_eq!(subs.len(), 2);
        assert_eq!(per.len(), 3);
    }
    #[test]
    fn entropy_is_zero_for_a_single_file_change() {
        assert_eq!(shannon_entropy(&[42.0]), 0.0);
    }
    #[test]
    fn entropy_is_positive_for_a_scattered_change() {
        assert!(shannon_entropy(&[10.0, 10.0, 10.0, 10.0]) > 1.9);
    }
    #[test]
    fn a_small_focused_commit_scores_lower_than_a_large_scattered_one() {
        let reference = (1..=20)
            .map(|n| shape(n as f64 * 10.0, n as f64, Some(n as f64)))
            .collect::<Vec<_>>();
        assert!(
            score_commit(&shape(3.0, 1.0, Some(50.0)), &reference).score
                < score_commit(&shape(400.0, 25.0, Some(1.0)), &reference).score
        );
    }
    #[test]
    fn unknown_author_experience_is_reported_without_changing_other_factors() {
        let reference = (1..=5)
            .map(|n| shape(n as f64, n as f64, Some(n as f64)))
            .collect::<Vec<_>>();
        let risk = score_commit(&shape(3.0, 2.0, None), &reference);
        let factor = risk
            .factors
            .iter()
            .find(|factor| factor.feature == "author_experience")
            .unwrap();
        assert!(factor.percentile.is_none());
        assert_eq!(factor.contribution, 0.0);
    }
}
