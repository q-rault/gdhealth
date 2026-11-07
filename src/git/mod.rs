mod blame;
mod exec;
mod hotspot;
mod log_walk;
pub mod risk;

use crate::config::GitConfig;
use crate::model::{GitFileStats, GitIntelligenceReport, GitOwnershipShare, ProjectFile};
use exec::{GitError, run_git_text};
use std::collections::BTreeSet;
use std::path::Path;

/// Collects git-derived signals (churn hotspots, ownership, co-change,
/// change entropy, prior-defect history, bus factor) for `files`. Never
/// returns an error to its caller: any failure (missing git binary, not a
/// repository, no commits yet) degrades to
/// `GitIntelligenceReport { available: false, .. }` so the rest of
/// `gdhealth health` keeps working.
pub fn collect(root: &Path, files: &[ProjectFile], config: &GitConfig) -> GitIntelligenceReport {
    match collect_inner(root, files, config) {
        Ok(report) => report,
        Err(error) => unavailable_report(error.to_string()),
    }
}

fn collect_inner(
    root: &Path,
    files: &[ProjectFile],
    config: &GitConfig,
) -> Result<GitIntelligenceReport, GitError> {
    run_git_text(root, &["rev-parse", "--is-inside-work-tree"])?;

    let shallow_repository = run_git_text(root, &["rev-parse", "--is-shallow-repository"])
        .map(|text| text.trim() == "true")
        .unwrap_or(false);

    let anchor_time = match run_git_text(root, &["log", "-1", "--format=%at"]) {
        Ok(text) if !text.trim().is_empty() => text.trim().parse::<i64>().ok(),
        _ => None,
    };

    let Some(anchor_time) = anchor_time else {
        // A valid, non-shallow repository with zero commits yet.
        return Ok(GitIntelligenceReport {
            available: true,
            unavailable_reason: None,
            shallow_repository,
            anchor_commit_unix_time: None,
            repo_bus_factor: 0,
            repo_bus_factor_contributors: Vec::new(),
            files: Vec::new(),
            co_change_pairs: Vec::new(),
            warnings: Vec::new(),
        });
    };

    let commits = log_walk::walk_commits(root)?;
    let known_files = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();

    let mut file_stats = hotspot::compute_file_stats(&commits, &known_files, config, anchor_time);
    let (repo_bus_factor, repo_bus_factor_contributors) = hotspot::compute_bus_factor(&commits);
    let co_change_pairs =
        hotspot::compute_co_change_pairs(&commits, &known_files, config, anchor_time);

    let mut warnings = Vec::new();
    for file in files {
        if file.line_count > config.blame_max_file_lines {
            continue;
        }
        match blame::blame_file(root, &file.path) {
            Ok(shares) => {
                if let Some(entry) = file_stats.get_mut(&file.path) {
                    entry.ownership = ownership_shares(shares);
                }
            }
            Err(error) => warnings.push(format!("{}: blame failed: {error}", file.path)),
        }
    }

    hotspot::apply_percentiles(&mut file_stats);

    let mut files_out = known_files
        .iter()
        .map(|path| match file_stats.remove(path) {
            Some(stats) => GitFileStats {
                file: path.clone(),
                commit_count_total: stats.commit_count_total,
                commit_count_90d: stats.commit_count_90d,
                commit_count_30d: stats.commit_count_30d,
                lines_added_90d: stats.lines_added_90d,
                lines_deleted_90d: stats.lines_deleted_90d,
                temporal_hotspot_score: stats.temporal_hotspot_score,
                change_entropy: stats.change_entropy,
                prior_defect_count: stats.prior_defect_count,
                ownership: stats.ownership,
                churn_percentile: stats.churn_percentile,
                is_churn_hotspot: stats.is_churn_hotspot,
            },
            None => empty_file_stats(path.clone()),
        })
        .collect::<Vec<_>>();
    files_out.sort_by(|left, right| left.file.cmp(&right.file));

    Ok(GitIntelligenceReport {
        available: true,
        unavailable_reason: None,
        shallow_repository,
        anchor_commit_unix_time: Some(anchor_time),
        repo_bus_factor,
        repo_bus_factor_contributors,
        files: files_out,
        co_change_pairs,
        warnings,
    })
}

fn ownership_shares(blame: Vec<blame::BlameShare>) -> Vec<GitOwnershipShare> {
    let total: usize = blame.iter().map(|share| share.lines).sum();
    if total == 0 {
        return Vec::new();
    }

    blame
        .into_iter()
        .map(|share| GitOwnershipShare {
            share: share.lines as f64 / total as f64,
            author_email: share.author_email,
        })
        .collect()
}

fn empty_file_stats(file: String) -> GitFileStats {
    GitFileStats {
        file,
        commit_count_total: 0,
        commit_count_90d: 0,
        commit_count_30d: 0,
        lines_added_90d: 0,
        lines_deleted_90d: 0,
        temporal_hotspot_score: 0.0,
        change_entropy: 0.0,
        prior_defect_count: 0,
        ownership: Vec::new(),
        churn_percentile: 0.0,
        is_churn_hotspot: false,
    }
}

fn unavailable_report(reason: String) -> GitIntelligenceReport {
    <GitIntelligenceReport as crate::isolate::Diagnosable>::unavailable(reason)
}

#[cfg(test)]
mod tests {
    use super::collect;
    use crate::config::GitConfig;
    use crate::model::ProjectFile;
    use std::path::Path;

    #[test]
    fn collect_degrades_gracefully_when_not_a_git_repository() {
        let root =
            std::env::temp_dir().join(format!("gdhealth-git-not-a-repo-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(root.join("main.gd"), "extends RefCounted\n").expect("write fixture");

        let files = vec![ProjectFile {
            id: "main.gd".to_string(),
            path: "main.gd".to_string(),
            group: None,
            line_count: 1,
        }];

        let report = collect(&root, &files, &GitConfig::default());
        assert!(!report.available);
        assert!(report.unavailable_reason.is_some());

        std::fs::remove_dir_all(&root).ok();
        let _ = Path::new(&root);
    }
}
