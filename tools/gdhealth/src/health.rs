use crate::cli::CommandOptions;
use crate::config::Config;
use crate::error::Result;
use crate::graph::build_graph;
use crate::metrics::compute_metrics;
use crate::model::{HealthReport, HealthSummary, OutputMetadata};
use crate::policy::collect_findings;

pub fn build_health_report(config: &Config, options: &CommandOptions) -> Result<HealthReport> {
    let graph = build_graph(config, options)?;
    let metrics = compute_metrics(config, options)?;
    let policy_findings = collect_findings(config, &graph, &metrics)?;

    Ok(HealthReport {
        metadata: OutputMetadata::default(),
        summary: HealthSummary {
            file_count: graph.file_count,
            edge_count: graph.edges.len(),
            policy_finding_count: policy_findings.len(),
        },
        top_files: metrics.files.into_iter().take(10).collect(),
        top_functions: metrics.functions.into_iter().take(10).collect(),
        cycles: graph.cycles,
        policy_findings,
        cross_group_dependencies: Vec::new(),
    })
}
