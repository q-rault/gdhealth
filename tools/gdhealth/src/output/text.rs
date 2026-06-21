use crate::model::{
    FileMetric, FunctionMetric, GraphReport, HealthReport, MetricsReport, PolicyFinding,
    PolicyReport,
};

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
        format!("cycles: {}", report.cycles.len()),
        format!(
            "cross-group dependencies: {}",
            report.cross_group_dependencies.len()
        ),
        format!("policy findings: {}", report.summary.policy_finding_count),
    ];

    extend_top_files(&mut lines, &report.top_files);
    extend_top_functions(&mut lines, &report.top_functions);
    extend_cross_group_dependencies(&mut lines, &report.cross_group_dependencies);
    extend_cycles(&mut lines, &report.cycles);
    extend_policy_findings(&mut lines, &report.policy_findings);

    lines.join("\n")
}

pub fn render_health_bundle_summary(
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy: &PolicyReport,
    health: &HealthReport,
) -> String {
    let mut lines = vec![
        format!("root: {}", graph.root),
        format!("files: {}", health.summary.file_count),
        format!("edges: {}", health.summary.edge_count),
        format!("cycles: {}", health.cycles.len()),
        format!(
            "cross-group dependencies: {}",
            health.cross_group_dependencies.len()
        ),
        format!("policy findings: {}", health.summary.policy_finding_count),
        format!("parser warnings: {}", graph.parser_warnings.len()),
    ];

    extend_top_files(&mut lines, &health.top_files);
    extend_top_functions(&mut lines, &health.top_functions);
    extend_cross_group_dependencies(&mut lines, &health.cross_group_dependencies);
    extend_cycles(&mut lines, &health.cycles);
    extend_policy_findings(&mut lines, &policy.findings);

    if !graph.parser_warnings.is_empty() {
        lines.push("parser warnings:".to_string());
        lines.extend(
            graph
                .parser_warnings
                .iter()
                .take(10)
                .map(|warning| format!("- {warning}")),
        );
    }

    if metrics.files.len() > health.top_files.len() {
        lines.push(format!(
            "truncated top files to {} of {} entries",
            health.top_files.len(),
            metrics.files.len()
        ));
    }

    if metrics.functions.len() > health.top_functions.len() {
        lines.push(format!(
            "truncated top functions to {} of {} entries",
            health.top_functions.len(),
            metrics.functions.len()
        ));
    }

    lines.join("\n")
}

fn extend_top_files(lines: &mut Vec<String>, files: &[FileMetric]) {
    lines.push("top files:".to_string());
    lines.extend(files.iter().map(|file| {
        format!(
            "- {} (hotspot: {}, lines: {}, funcs: {}, incoming: {}, outgoing: {}, cycles: {})",
            file.file,
            file.file_hotspot_score,
            file.line_count,
            file.function_count,
            file.incoming_dependency_count,
            file.outgoing_dependency_count,
            file.cycle_count
        )
    }));
}

fn extend_top_functions(lines: &mut Vec<String>, functions: &[FunctionMetric]) {
    if functions.is_empty() {
        return;
    }

    lines.push("top functions:".to_string());
    lines.extend(functions.iter().map(|function| {
        format!(
            "- {}::{} (hotspot: {}, lines: {}, cyclomatic: {}, nesting: {}, params: {})",
            function.file,
            function.function_name,
            function.hotspot_score,
            function.line_count,
            function.cyclomatic_estimate,
            function.max_nesting_depth,
            function.parameter_count
        )
    }));
}

fn extend_cross_group_dependencies(lines: &mut Vec<String>, dependencies: &[String]) {
    if dependencies.is_empty() {
        return;
    }

    lines.push("cross-group dependencies:".to_string());
    lines.extend(
        dependencies
            .iter()
            .take(10)
            .map(|dependency| format!("- {dependency}")),
    );
}

fn extend_cycles(lines: &mut Vec<String>, cycles: &[Vec<String>]) {
    if cycles.is_empty() {
        return;
    }

    lines.push("cycles:".to_string());
    lines.extend(
        cycles
            .iter()
            .map(|cycle| format!("- {}", cycle.join(" -> "))),
    );
}

fn extend_policy_findings(lines: &mut Vec<String>, findings: &[PolicyFinding]) {
    if findings.is_empty() {
        return;
    }

    lines.push("policy findings:".to_string());
    lines.extend(findings.iter().take(20).map(|finding| {
        format!(
            "- [{}] {}: {}",
            finding.severity, finding.rule_id, finding.message
        )
    }));
}
