//! Change-risk scoring for a commit or `base..head` range.
//!
//! Kamei-style diff-shape features (size, diffusion, author experience),
//! scored with the exact calibrated constants published by repowise
//! (`packages/core/.../change_risk/model.py`, 7-repo/5-language corpus,
//! leave-one-repo-out AUC 0.772). Ported verbatim as a placeholder — these
//! were never fit on a Godot/GDScript corpus, so every output is labeled
//! uncalibrated. The scoring math itself is language-agnostic (pure
//! diff-shape statistics), so the algorithm is trustworthy even though the
//! specific coefficients are borrowed.

use super::exec::{GitError, run_git_text};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ChangeFeatures {
    pub la: f64,
    pub ld: f64,
    pub nf: f64,
    pub nd: f64,
    pub ns: f64,
    pub entropy: f64,
    pub exp: Option<f64>,
    pub author: String,
    pub subject: String,
    pub ref_label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskDriver {
    pub feature: String,
    /// `None` when the feature is unknown for this change (e.g. author
    /// experience with no resolvable git history) — serde_json would
    /// reject a raw NaN, and `None` is also the more honest JSON shape.
    pub value: Option<f64>,
    pub contribution: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeRisk {
    pub score: f64,
    pub probability: f64,
    pub level: String,
    pub drivers: Vec<RiskDriver>,
    pub ref_label: String,
    pub calibration_warning: &'static str,
}

const FEATURE_NAMES: [&str; 7] = ["la", "ld", "nf", "nd", "ns", "entropy", "exp"];
const LOG1P: [bool; 7] = [true, true, true, true, true, false, true];
const MEAN: [f64; 7] = [2.443, 1.723, 1.006, 0.795, 0.806, 0.537, 3.011];
const STD: [f64; 7] = [1.414, 1.380, 0.465, 0.237, 0.285, 0.776, 2.270];
const COEF: [f64; 7] = [1.1241, 0.0151, -0.1103, -0.0310, -0.0672, 0.1483, -0.0702];
const INTERCEPT: f64 = -0.3797;

/// Scores a commit (no `..` in `revspec`) or a `base..head` range.
pub fn score_revspec(root: &Path, revspec: &str) -> Result<ChangeRisk, String> {
    let features = if let Some((base, head)) = revspec.split_once("..") {
        extract_range_features(root, base, head)
    } else {
        extract_commit_features(root, revspec)
    }
    .map_err(|error| error.to_string())?;

    Ok(score_change(&features))
}

fn extract_commit_features(root: &Path, sha: &str) -> Result<ChangeFeatures, GitError> {
    let meta = run_git_text(root, &["show", "-s", "--format=%an%x00%s", sha])?;
    let (author, subject) = split_author_subject(&meta);
    let numstat = run_git_text(root, &["show", sha, "--numstat", "--format="])?;
    let (la, ld, nf, dirs, subs, per_file) = accumulate_numstat(&numstat);

    let parent = run_git_text(root, &["rev-parse", "--verify", "--quiet", &format!("{sha}^")])
        .unwrap_or_default();
    let parent = parent.trim();
    let upto_ref = if parent.is_empty() { sha } else { parent };
    let exp = author_experience(root, &author, upto_ref);

    Ok(ChangeFeatures {
        la,
        ld,
        nf,
        nd: dirs.len() as f64,
        ns: subs.len() as f64,
        entropy: shannon_entropy(&per_file),
        exp,
        author,
        subject,
        ref_label: sha.to_string(),
    })
}

fn extract_range_features(root: &Path, base: &str, head: &str) -> Result<ChangeFeatures, GitError> {
    let revspec = format!("{base}..{head}");
    let numstat = run_git_text(root, &["diff", "--numstat", &revspec])?;
    let (la, ld, nf, dirs, subs, per_file) = accumulate_numstat(&numstat);

    let meta = run_git_text(root, &["show", "-s", "--format=%an%x00%s", head])?;
    let (author, subject) = split_author_subject(&meta);
    let exp = author_experience(root, &author, base);

    Ok(ChangeFeatures {
        la,
        ld,
        nf,
        nd: dirs.len() as f64,
        ns: subs.len() as f64,
        entropy: shannon_entropy(&per_file),
        exp,
        author,
        subject,
        ref_label: revspec,
    })
}

fn accumulate_numstat(numstat: &str) -> (f64, f64, f64, BTreeSet<String>, BTreeSet<String>, Vec<f64>) {
    let mut la = 0.0;
    let mut ld = 0.0;
    let mut nf = 0.0;
    let mut dirs = BTreeSet::new();
    let mut subs = BTreeSet::new();
    let mut per_file = Vec::new();

    for row in numstat.lines() {
        let row = row.trim();
        if row.is_empty() {
            continue;
        }
        let mut parts = row.splitn(3, '\t');
        let added_raw = parts.next().unwrap_or("0");
        let deleted_raw = parts.next().unwrap_or("0");
        let path = parts.next().unwrap_or("").trim();
        if path.is_empty() {
            continue;
        }

        let added: f64 = added_raw.parse().unwrap_or(0.0);
        let deleted: f64 = deleted_raw.parse().unwrap_or(0.0);
        la += added;
        ld += deleted;
        nf += 1.0;
        let churn = added + deleted;
        if churn > 0.0 {
            per_file.push(churn);
        }

        let segments = path.split('/').collect::<Vec<_>>();
        let dir = segments[..segments.len().saturating_sub(1)].join("/");
        dirs.insert(dir);
        subs.insert(segments[0].to_string());
    }

    (la, ld, nf, dirs, subs, per_file)
}

fn shannon_entropy(per_file: &[f64]) -> f64 {
    let total: f64 = per_file.iter().sum();
    if total <= 0.0 || per_file.len() < 2 {
        return 0.0;
    }

    -per_file
        .iter()
        .filter(|&&value| value > 0.0)
        .map(|&value| {
            let share = value / total;
            share * share.log2()
        })
        .sum::<f64>()
}

fn author_experience(root: &Path, author: &str, upto_ref: &str) -> Option<f64> {
    if author.is_empty() {
        return None;
    }
    let output = run_git_text(
        root,
        &["rev-list", "--count", "--author", author, "--no-merges", upto_ref],
    )
    .ok()?;
    output.trim().parse::<f64>().ok()
}

fn split_author_subject(text: &str) -> (String, String) {
    let text = text.trim_end_matches('\n');
    match text.split_once('\u{0}') {
        Some((author, subject)) => (author.to_string(), subject.to_string()),
        None => (text.to_string(), String::new()),
    }
}

fn score_change(features: &ChangeFeatures) -> ChangeRisk {
    let raw_values: [Option<f64>; 7] = [
        Some(features.la),
        Some(features.ld),
        Some(features.nf),
        Some(features.nd),
        Some(features.ns),
        Some(features.entropy),
        features.exp,
    ];

    let mut logit = INTERCEPT;
    let mut drivers = Vec::with_capacity(FEATURE_NAMES.len());

    for i in 0..FEATURE_NAMES.len() {
        let name = FEATURE_NAMES[i];
        match raw_values[i] {
            None => drivers.push(RiskDriver {
                feature: name.to_string(),
                value: None,
                contribution: 0.0,
                label: format!("{name} (unknown)"),
            }),
            Some(raw) => {
                let x = if LOG1P[i] { raw.ln_1p() } else { raw };
                let std = if STD[i] != 0.0 { STD[i] } else { 1.0 };
                let z = (x - MEAN[i]) / std;
                let contribution = COEF[i] * z;
                logit += contribution;
                drivers.push(RiskDriver {
                    feature: name.to_string(),
                    value: Some(raw),
                    contribution,
                    label: feature_label(name, x >= MEAN[i]),
                });
            }
        }
    }

    let probability = sigmoid(logit);
    let score = (10.0 * probability * 10.0).round() / 10.0;

    ChangeRisk {
        score,
        probability,
        level: level_for(score),
        drivers,
        ref_label: features.ref_label.clone(),
        calibration_warning: "coefficients are ported from repowise's generic (non-GDScript) corpus — treat as directional, not absolute",
    }
}

fn sigmoid(z: f64) -> f64 {
    if z >= 0.0 {
        1.0 / (1.0 + (-z).exp())
    } else {
        let e = z.exp();
        e / (1.0 + e)
    }
}

fn level_for(score: f64) -> String {
    if score >= 7.0 {
        "high".to_string()
    } else if score >= 4.0 {
        "moderate".to_string()
    } else {
        "low".to_string()
    }
}

fn feature_label(name: &str, above_baseline: bool) -> String {
    let (above, below) = match name {
        "la" => ("more lines added than baseline", "fewer lines added than baseline"),
        "ld" => ("more lines deleted than baseline", "fewer lines deleted than baseline"),
        "nf" => ("more files than baseline", "fewer files than baseline"),
        "nd" => ("more directories than baseline", "fewer directories than baseline"),
        "ns" => ("more subsystems than baseline", "fewer subsystems than baseline"),
        "entropy" => ("more scattered than baseline", "more focused than baseline"),
        "exp" => (
            "more experienced author than baseline",
            "less familiar author than baseline",
        ),
        _ => (name, name),
    };
    if above_baseline { above } else { below }.to_string()
}

#[cfg(test)]
mod tests {
    use super::{ChangeFeatures, accumulate_numstat, score_change, shannon_entropy};

    #[test]
    fn accumulates_numstat_into_size_diffusion_and_per_file_churn() {
        let numstat = "10\t2\tgameplay/core/foo.gd\n5\t0\tgameplay/combat/bar.gd\n0\t3\tnet/baz.gd\n";
        let (la, ld, nf, dirs, subs, per_file) = accumulate_numstat(numstat);
        assert_eq!(la, 15.0);
        assert_eq!(ld, 5.0);
        assert_eq!(nf, 3.0);
        assert_eq!(dirs.len(), 3);
        assert_eq!(subs.len(), 2); // "gameplay" and "net"
        assert_eq!(per_file.len(), 3);
    }

    #[test]
    fn entropy_is_zero_for_a_single_file_change() {
        assert_eq!(shannon_entropy(&[42.0]), 0.0);
    }

    #[test]
    fn entropy_is_positive_for_a_scattered_change() {
        let entropy = shannon_entropy(&[10.0, 10.0, 10.0, 10.0]);
        assert!(entropy > 1.9 && entropy < 2.1); // log2(4) == 2.0 for a uniform split
    }

    #[test]
    fn a_small_focused_commit_scores_lower_than_a_large_scattered_one() {
        let small = ChangeFeatures {
            la: 3.0,
            ld: 1.0,
            nf: 1.0,
            nd: 1.0,
            ns: 1.0,
            entropy: 0.0,
            exp: Some(50.0),
            author: "dev".to_string(),
            subject: "small fix".to_string(),
            ref_label: "abc".to_string(),
        };
        let large = ChangeFeatures {
            la: 400.0,
            ld: 300.0,
            nf: 25.0,
            nd: 10.0,
            ns: 5.0,
            entropy: 3.5,
            exp: Some(1.0),
            author: "newdev".to_string(),
            subject: "big rewrite".to_string(),
            ref_label: "def".to_string(),
        };

        let small_risk = score_change(&small);
        let large_risk = score_change(&large);
        assert!(
            large_risk.score > small_risk.score,
            "large scattered commit ({}) should score higher than small focused one ({})",
            large_risk.score,
            small_risk.score
        );
    }

    #[test]
    fn unknown_author_experience_contributes_zero_not_a_penalty() {
        let mut features = ChangeFeatures {
            la: 10.0,
            ld: 5.0,
            nf: 2.0,
            nd: 1.0,
            ns: 1.0,
            entropy: 1.0,
            exp: Some(3.0),
            author: "dev".to_string(),
            subject: "work".to_string(),
            ref_label: "abc".to_string(),
        };
        let with_exp = score_change(&features);
        features.exp = None;
        let without_exp = score_change(&features);

        let exp_driver = with_exp
            .drivers
            .iter()
            .find(|driver| driver.feature == "exp")
            .expect("exp driver present");
        let unknown_driver = without_exp
            .drivers
            .iter()
            .find(|driver| driver.feature == "exp")
            .expect("exp driver present");
        assert_eq!(unknown_driver.contribution, 0.0);
        assert_ne!(exp_driver.contribution, 0.0);
    }
}
