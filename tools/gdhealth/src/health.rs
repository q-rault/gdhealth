use crate::cli::CommandOptions;
use crate::config::Config;
use crate::error::Result;
use crate::fs;
use crate::graph::build_graph;
use crate::metrics::compute_metrics;
use crate::model::{
    GraphReport, HealthReport, HealthSummary, MetricsReport, OutputMetadata, PolicyReport,
};
use crate::output::{dot, json, text};
use crate::policy::collect_findings;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct HealthArtifacts {
    pub graph: GraphReport,
    pub metrics: MetricsReport,
    pub policy: PolicyReport,
    pub health: HealthReport,
}

pub fn build_health_artifacts(
    config: &Config,
    options: &CommandOptions,
) -> Result<HealthArtifacts> {
    let graph = build_graph(config, options)?;
    let metrics = compute_metrics(config, options)?;
    let policy_findings = collect_findings(config, &graph, &metrics)?;
    let cross_group_dependencies = collect_cross_group_dependencies(&graph);

    let policy = PolicyReport {
        metadata: OutputMetadata::default(),
        root: options.root.to_string_lossy().into_owned(),
        findings: policy_findings.clone(),
    };

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
    };

    Ok(HealthArtifacts {
        graph,
        metrics,
        policy,
        health,
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
