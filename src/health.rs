use crate::cli::CommandOptions;
use crate::config::Config;
use crate::error::Result;
use crate::fs;
use crate::git;
use crate::graph::build_graph;
use crate::isolate::isolate;
use crate::metrics::compute_metrics;
use crate::model::{
    GitIntelligenceReport, GraphReport, HealthReport, HealthSummary, MetricsReport,
    OutputMetadata, PolicyReport,
};
use crate::output::{browser, dot, json, text};
use crate::policy::collect_findings;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct HealthArtifacts {
    pub graph: GraphReport,
    pub metrics: MetricsReport,
    pub policy: PolicyReport,
    pub health: HealthReport,
    pub git: GitIntelligenceReport,
}

pub fn build_health_artifacts(
    config: &Config,
    options: &CommandOptions,
) -> Result<HealthArtifacts> {
    let graph = build_graph(config, options)?;
    let metrics = compute_metrics(config, options)?;
    let policy_findings = collect_findings(config, &graph, &metrics)?;
    let cross_group_dependencies = collect_cross_group_dependencies(&graph);

    let git_report = if config.git.enabled && !options.no_git {
        isolate("git-intelligence", || {
            git::collect(options.root.as_path(), &graph.files, &config.git)
        })
    } else {
        <GitIntelligenceReport as crate::isolate::Diagnosable>::unavailable(
            "git intelligence disabled (--no-git or [git] enabled = false)".to_string(),
        )
    };

    let policy = PolicyReport {
        metadata: OutputMetadata::default(),
        root: options.root.to_string_lossy().into_owned(),
        findings: policy_findings.clone(),
    };

    let mut top_churn_files = git_report.files.clone();
    top_churn_files.sort_by(|left, right| {
        right
            .temporal_hotspot_score
            .partial_cmp(&left.temporal_hotspot_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.file.cmp(&right.file))
    });
    top_churn_files.truncate(10);

    let mut top_co_changes = git_report.co_change_pairs.clone();
    top_co_changes.truncate(10);

    let health = HealthReport {
        metadata: OutputMetadata::default(),
        summary: HealthSummary {
            file_count: graph.file_count,
            edge_count: graph.edges.len(),
            policy_finding_count: policy_findings.len(),
        },
        top_files: metrics.files.iter().take(10).cloned().collect(),
        top_functions: metrics.functions.iter().take(10).cloned().collect(),
        cycles: graph.cycles.clone(),
        policy_findings,
        cross_group_dependencies,
        git_available: git_report.available,
        git_unavailable_reason: git_report.unavailable_reason.clone(),
        git_shallow_repository: git_report.shallow_repository,
        repo_bus_factor: git_report.repo_bus_factor,
        top_churn_files,
        top_co_changes,
    };

    Ok(HealthArtifacts {
        graph,
        metrics,
        policy,
        health,
        git: git_report,
    })
}

pub fn build_health_report(config: &Config, options: &CommandOptions) -> Result<HealthReport> {
    Ok(build_health_artifacts(config, options)?.health)
}

pub fn default_bundle_dir(root: &Path) -> PathBuf {
    root.join("reports").join("code-health")
}

pub fn emit_report_bundle(bundle_dir: &Path, artifacts: &HealthArtifacts) -> Result<()> {
    let outputs = [
        (
            bundle_dir.join("graph.json"),
            json::render(&artifacts.graph)?,
        ),
        (
            bundle_dir.join("graph.dot"),
            dot::render_graph(&artifacts.graph),
        ),
        (
            bundle_dir.join("metrics.json"),
            json::render(&artifacts.metrics)?,
        ),
        (
            bundle_dir.join("policy.json"),
            json::render(&artifacts.policy)?,
        ),
        (
            bundle_dir.join("health.json"),
            json::render(&artifacts.health)?,
        ),
        (
            bundle_dir.join("summary.txt"),
            text::render_health_bundle_summary(
                &artifacts.graph,
                &artifacts.metrics,
                &artifacts.policy,
                &artifacts.health,
            ),
        ),
    ];

    for (path, content) in outputs {
        fs::write_output(&path, &content)?;
    }

    let root = PathBuf::from(&artifacts.graph.root);
    let browser_dir = bundle_dir.join("browser");
    let cross_group_dep_count = artifacts.health.cross_group_dependencies.len();
    browser::emit_emerge_html(
        &root,
        Some(&browser_dir),
        &artifacts.graph,
        &artifacts.metrics,
        &artifacts.policy.findings,
        cross_group_dep_count,
    )?;

    Ok(())
}

fn collect_cross_group_dependencies(graph: &GraphReport) -> Vec<String> {
    let group_by_file = graph
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.group.as_deref()))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = graph
        .edges
        .iter()
        .filter_map(|edge| {
            let from_group = group_by_file.get(edge.from.as_str()).copied().flatten();
            let to_group = group_by_file.get(edge.to.as_str()).copied().flatten();
            if from_group == to_group {
                return None;
            }

            Some(format!(
                "{} -> {} ({} -> {})",
                edge.from,
                edge.to,
                from_group.unwrap_or("ungrouped"),
                to_group.unwrap_or("ungrouped")
            ))
        })
        .collect::<Vec<_>>();

    dependencies.sort();
    dependencies.dedup();
    dependencies
}
