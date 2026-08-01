use crate::defect_validation::DefectValidationReport;
use crate::git::risk::CommitRiskScore;
use crate::model::{
    CoChangePair, FileMetric, FunctionMetric, GitFileStats, GraphReport, HealthReport,
    MetricsReport, PolicyFinding, PolicyReport,
};

pub fn render_graph(report: &GraphReport) -> String {
    let mut lines = vec![
        format!("root: {}", report.root),
        format!("files: {}", report.file_count),
        format!("edges: {}", report.edges.len()),
        format!("cycles: {}", report.cycles.len()),
        format!("parser warnings: {}", report.parser_warnings.len()),
        format!(
            "entity scan: {} entities",
            report
                .entity_scan
                .as_ref()
                .map(|scan| scan.entities.len())
                .unwrap_or(0)
        ),
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
        format!("entities analyzed: {}", report.entities.len()),
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

    if !report.entities.is_empty() {
        lines.push("top entities:".to_string());
        lines.extend(report.entities.iter().take(10).map(|entity| {
            format!(
                "- {} (file: {}, methods: {}, ws: {}, fan-in: {}, fan-out: {})",
                entity.entity,
                entity.file,
                entity.method_count,
                entity.ws_complexity,
                entity.incoming_dependency_count,
                entity.outgoing_dependency_count
            )
        }));
    }

    extend_duplication(&mut lines, &report.duplication);

    lines.join("\n")
}

fn extend_duplication(lines: &mut Vec<String>, duplication: &crate::model::DuplicationReport) {
    if !duplication.available {
        return;
    }
    if duplication.clones.is_empty() {
        return;
    }

    lines.push(format!(
        "duplicate code blocks: {}",
        duplication.clones.len()
    ));
    lines.extend(duplication.clones.iter().take(20).map(|clone| {
        format!(
            "- {}:{} <-> {}:{} ({} tokens)",
            clone.file_a, clone.start_line_a, clone.file_b, clone.start_line_b, clone.token_count
        )
    }));
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
    extend_git_signals(
        &mut lines,
        report.git_available,
        report.git_unavailable_reason.as_deref(),
        report.repo_bus_factor,
        &report.top_churn_files,
        &report.top_co_changes,
    );

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
    extend_duplication(&mut lines, &metrics.duplication);
    extend_git_signals(
        &mut lines,
        health.git_available,
        health.git_unavailable_reason.as_deref(),
        health.repo_bus_factor,
        &health.top_churn_files,
        &health.top_co_changes,
    );

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

pub fn render_risk(risk: &CommitRiskScore) -> String {
    let mut lines = vec![
        format!("ref: {}", risk.ref_label),
        format!("risk score: {:.1}/10 ({})", risk.score, risk.level),
        format!("reference commits: {}", risk.reference_commit_count),
        format!("note: {}", risk.method_note),
        "factors:".to_string(),
    ];

    let mut sorted_factors = risk.factors.clone();
    sorted_factors.sort_by(|left, right| {
        right
            .contribution
            .abs()
            .partial_cmp(&left.contribution.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    lines.extend(sorted_factors.iter().map(|factor| match factor.value {
        Some(value) => format!(
            "- {}: {value:.1} ({}, weight: {:.3})",
            factor.feature, factor.label, factor.contribution
        ),
        None => format!("- {}: {}", factor.feature, factor.label),
    }));

    lines.join("\n")
}

pub fn render_defect_validation(report: &DefectValidationReport) -> String {
    if !report.available {
        return format!(
            "defect validation: unavailable ({})",
            report.reason.as_deref().unwrap_or("unknown reason")
        );
    }

    format!(
        "defect validation: {}/{} of the {} worst-hotspot files had a bug fix in the tracked window, {:.0}% vs a {:.0}% repo baseline ({:.1}x lift)\nscored files: {}, files with prior-defect history: {}",
        report.hits,
        report.bottom_k,
        report.bottom_k,
        report.precision * 100.0,
        report.base_rate * 100.0,
        report.lift,
        report.scored_file_count,
        report.defect_file_count
    )
}

fn extend_git_signals(
    lines: &mut Vec<String>,
    git_available: bool,
    git_unavailable_reason: Option<&str>,
    repo_bus_factor: usize,
    top_churn_files: &[GitFileStats],
    top_co_changes: &[CoChangePair],
) {
    lines.push("git signals:".to_string());

    if !git_available {
        let reason = git_unavailable_reason.unwrap_or("unknown reason");
        lines.push(format!("- unavailable: {reason}"));
        return;
    }

    lines.push(format!("- repo bus factor: {repo_bus_factor}"));

    if !top_churn_files.is_empty() {
        lines.push("- most active files (90d):".to_string());
        lines.extend(top_churn_files.iter().map(|file| {
            let owner = file
                .ownership
                .first()
                .map(|share| format!("{} ({:.0}%)", share.author_email, share.share * 100.0))
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "  - {} (hotspot: {:.1}, commits/90d: {}, prior defects: {}, owner: {})",
                file.file,
                file.temporal_hotspot_score,
                file.commit_count_90d,
                file.prior_defect_count,
                owner
            )
        }));
    }

    if !top_co_changes.is_empty() {
        lines.push("- co-change highlights:".to_string());
        lines.extend(top_co_changes.iter().map(|pair| {
            format!(
                "  - {} <-> {} (weight: {:.2})",
                pair.file_a, pair.file_b, pair.weight
            )
        }));
    }
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
