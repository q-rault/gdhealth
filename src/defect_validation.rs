//! Self-check: does gdhealth's own structural hotspot score actually line
//! up with files that had real bugs? It uses a bottom-K precision/lift check
//! adapted to gdhealth's data model — gdhealth has no unified 1-10 health
//! score, so `file_hotspot_score`
//! (already computed from complexity + coupling) stands in as the "how
//! risky is this file" ranking signal, and prior-defect history comes from
//! the git intelligence layer.

use crate::model::{FileMetric, GitIntelligenceReport};
use serde::Serialize;
use std::collections::BTreeMap;

const MIN_SCORED_FILES: usize = 25;
const MIN_DEFECT_FILES: usize = 5;
pub const DEFAULT_K: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct DefectValidationReport {
    pub available: bool,
    pub reason: Option<String>,
    pub scored_file_count: usize,
    pub defect_file_count: usize,
    pub bottom_k: usize,
    pub hits: usize,
    pub precision: f64,
    pub base_rate: f64,
    pub lift: f64,
}

impl DefectValidationReport {
    fn unavailable(reason: String) -> Self {
        Self {
            available: false,
            reason: Some(reason),
            scored_file_count: 0,
            defect_file_count: 0,
            bottom_k: 0,
            hits: 0,
            precision: 0.0,
            base_rate: 0.0,
            lift: 0.0,
        }
    }
}

pub fn validate(
    files: &[FileMetric],
    git: &GitIntelligenceReport,
    k: usize,
) -> DefectValidationReport {
    if !git.available {
        return DefectValidationReport::unavailable(
            "git intelligence unavailable — prior-defect history requires git".to_string(),
        );
    }

    let defect_count_by_file = git
        .files
        .iter()
        .map(|file| (file.file.as_str(), file.prior_defect_count))
        .collect::<BTreeMap<_, _>>();

    let scored_file_count = files.len();
    let defect_file_count = git
        .files
        .iter()
        .filter(|file| file.prior_defect_count > 0)
        .count();

    if scored_file_count < MIN_SCORED_FILES || defect_file_count < MIN_DEFECT_FILES {
        return DefectValidationReport::unavailable(format!(
            "insufficient data: {scored_file_count} scored files, {defect_file_count} with prior-defect history (need at least {MIN_SCORED_FILES} and {MIN_DEFECT_FILES})"
        ));
    }

    let mut ranked = files.to_vec();
    ranked.sort_by(|left, right| {
        right
            .file_hotspot_score
            .cmp(&left.file_hotspot_score)
            .then_with(|| left.file.cmp(&right.file))
    });

    let bottom_k = k.min(ranked.len());
    let hits = ranked
        .iter()
        .take(bottom_k)
        .filter(|file| {
            defect_count_by_file
                .get(file.file.as_str())
                .copied()
                .unwrap_or(0)
                > 0
        })
        .count();

    let precision = if bottom_k > 0 {
        hits as f64 / bottom_k as f64
    } else {
        0.0
    };
    let base_rate = defect_file_count as f64 / scored_file_count as f64;
    let lift = if base_rate > 0.0 {
        precision / base_rate
    } else {
        0.0
    };

    DefectValidationReport {
        available: true,
        reason: None,
        scored_file_count,
        defect_file_count,
        bottom_k,
        hits,
        precision,
        base_rate,
        lift,
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_K, MIN_DEFECT_FILES, MIN_SCORED_FILES, validate};
    use crate::model::{CoChangePair, FileMetric, GitFileStats, GitIntelligenceReport};

    fn file_metric(name: &str, hotspot_score: usize) -> FileMetric {
        FileMetric {
            file: name.to_string(),
            line_count: 10,
            function_count: 1,
            number_of_methods: 1,
            signal_count: 0,
            public_function_count: 1,
            outgoing_dependency_count: 0,
            incoming_dependency_count: 0,
            cycle_count: 0,
            ws_complexity: 1,
            tfidf: 0.0,
            louvain_modularity: 0,
            file_hotspot_score: hotspot_score,
        }
    }

    fn git_file(name: &str, prior_defect_count: usize) -> GitFileStats {
        GitFileStats {
            file: name.to_string(),
            commit_count_total: 1,
            commit_count_90d: 1,
            commit_count_30d: 0,
            lines_added_90d: 0,
            lines_deleted_90d: 0,
            temporal_hotspot_score: 0.0,
            change_entropy: 0.0,
            prior_defect_count,
            ownership: Vec::new(),
            churn_percentile: 0.0,
            is_churn_hotspot: false,
        }
    }

    fn available_git(files: Vec<GitFileStats>) -> GitIntelligenceReport {
        GitIntelligenceReport {
            available: true,
            unavailable_reason: None,
            shallow_repository: false,
            anchor_commit_unix_time: Some(0),
            repo_bus_factor: 1,
            repo_bus_factor_contributors: Vec::new(),
            files,
            co_change_pairs: Vec::<CoChangePair>::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn reports_unavailable_when_git_is_unavailable() {
        let files = (0..30)
            .map(|i| file_metric(&format!("f{i}.gd"), i))
            .collect::<Vec<_>>();
        let git = GitIntelligenceReport {
            available: false,
            ..available_git(Vec::new())
        };
        let report = validate(&files, &git, DEFAULT_K);
        assert!(!report.available);
    }

    #[test]
    fn reports_unavailable_below_minimum_thresholds() {
        let files = (0..10)
            .map(|i| file_metric(&format!("f{i}.gd"), i))
            .collect::<Vec<_>>();
        let git = available_git((0..10).map(|i| git_file(&format!("f{i}.gd"), 0)).collect());
        let report = validate(&files, &git, DEFAULT_K);
        assert!(!report.available);
        assert!(files.len() < MIN_SCORED_FILES);
    }

    #[test]
    fn computes_exact_precision_and_lift_on_a_known_fixture() {
        // 30 files, hotspot score = index (higher = worse), so the bottom-20
        // (worst-scored) are files 29..10. 6 of those 20 have defect history,
        // plus 2 more among the "healthy" files 9..0, so:
        // base_rate = 8/30, precision = 6/20, lift = precision / base_rate.
        let files = (0..30)
            .map(|i| file_metric(&format!("f{i}.gd"), i))
            .collect::<Vec<_>>();
        let mut git_files = Vec::new();
        for i in 0..30 {
            let defects = if (10..30).contains(&i) && i % 3 == 0 {
                1
            } else if i < 10 && (i == 1 || i == 3) {
                1
            } else {
                0
            };
            git_files.push(git_file(&format!("f{i}.gd"), defects));
        }
        let git = available_git(git_files);

        let report = validate(&files, &git, DEFAULT_K);
        assert!(report.available);
        assert_eq!(report.scored_file_count, 30);
        assert_eq!(report.bottom_k, 20);
        assert!(report.defect_file_count >= MIN_DEFECT_FILES);
        assert_eq!(report.hits, 6); // 12,15,18,21,24,27 -> 6 hits in top-20 worst
        assert!((report.precision - 0.3).abs() < 1e-9);
        let expected_lift = report.precision / report.base_rate;
        assert!((report.lift - expected_lift).abs() < 1e-9);
    }
}
