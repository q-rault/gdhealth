use crate::model::{GraphReport, HealthReport, MetricsReport, PolicyReport};

pub fn render_graph(report: &GraphReport) -> String {
    let mut lines = vec![
        format!("root: {}", report.root),
        format!("files: {}", report.file_count),
        format!("edges: {}", report.edges.len()),
        format!("cycles: {}", report.cycles.len()),
        format!("parser warnings: {}", report.parser_warnings.len()),
        "discovered files:".to_string(),
    ];

    lines.extend(
        report
            .files
            .iter()
            .map(|file| format!("- {} (lines: {})", file.path, file.line_count)),
    );

    if !report.edges.is_empty() {
        lines.push("dependencies:".to_string());
        lines.extend(report.edges.iter().take(20).map(|edge| {
            format!(
                "- {} -> {} ({:?} @ line {})",
                edge.from, edge.to, edge.kind, edge.line
            )
        }));
    }

    if !report.cycles.is_empty() {
        lines.push("cycles:".to_string());
        lines.extend(
            report
                .cycles
                .iter()
                .map(|cycle| format!("- {}", cycle.join(" -> "))),
        );
    }

    if !report.parser_warnings.is_empty() {
        lines.push("parser warnings:".to_string());
        lines.extend(
            report
                .parser_warnings
                .iter()
                .take(20)
                .map(|warning| format!("- {warning}")),
        );
    }

    lines.join("\n")
}

pub fn render_metrics(report: &MetricsReport) -> String {
    let mut lines = vec![
        format!("root: {}", report.root),
        format!("files analyzed: {}", report.files.len()),
        "top files:".to_string(),
    ];

    lines.extend(report.files.iter().take(10).map(|file| {
        format!(
            "- {} (lines: {}, hotspot: {})",
            file.file, file.line_count, file.file_hotspot_score
        )
    }));

    if !report.functions.is_empty() {
        lines.push("top functions:".to_string());
        lines.extend(report.functions.iter().take(10).map(|function| {
            format!(
                "- {}::{} (lines: {}, cyclomatic: {}, nesting: {}, hotspot: {})",
                function.file,
                function.function_name,
                function.line_count,
                function.cyclomatic_estimate,
                function.max_nesting_depth,
                function.hotspot_score
            )
        }));
    }

    lines.join("\n")
}

pub fn render_policy(report: &PolicyReport) -> String {
    let mut lines = vec![
        format!("root: {}", report.root),
        format!("findings: {}", report.findings.len()),
    ];

    lines.extend(report.findings.iter().map(|finding| {
        format!(
            "- [{}] {}: {}",
            finding.severity, finding.rule_id, finding.message
        )
    }));

    lines.join("\n")
}

pub fn render_health(report: &HealthReport) -> String {
    let mut lines = vec![
        format!("files: {}", report.summary.file_count),
        format!("edges: {}", report.summary.edge_count),
        format!("policy findings: {}", report.summary.policy_finding_count),
        "top files:".to_string(),
    ];

    lines.extend(report.top_files.iter().map(|file| {
        format!(
            "- {} (lines: {}, hotspot: {})",
            file.file, file.line_count, file.file_hotspot_score
        )
    }));

    lines.join("\n")
}
