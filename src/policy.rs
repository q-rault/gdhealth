use crate::cli::CommandOptions;
use crate::config::{Config, ForbiddenDependencyRule};
use crate::error::{GdHealthError, Result};
use crate::graph::build_graph;
use crate::metrics::compute_metrics;
use crate::model::{
    FileMetric, FunctionMetric, GraphReport, OutputMetadata, PolicyFinding, PolicyReport,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::BTreeMap;

pub fn evaluate_policy(config: &Config, options: &CommandOptions) -> Result<PolicyReport> {
    let graph = build_graph(config, options)?;
    let metrics = compute_metrics(config, options)?;
    let findings = collect_findings(config, &graph, &metrics)?;

    Ok(PolicyReport {
        metadata: OutputMetadata::default(),
        root: options.root.to_string_lossy().into_owned(),
        findings,
    })
}

pub fn enforce_fail_on(findings: &[PolicyFinding], fail_on: Option<&str>) -> Result<()> {
    let Some(raw_threshold) = fail_on else {
        return Ok(());
    };
    let threshold = Severity::parse(raw_threshold)?;
    let matched = findings
        .iter()
        .filter(|finding| {
            Severity::parse(&finding.severity).is_ok_and(|severity| severity >= threshold)
        })
        .count();

    if matched == 0 {
        return Ok(());
    }

    Err(GdHealthError::FailOnTriggered {
        message: format!("{matched} finding(s) at or above {threshold}",),
    })
}

pub fn collect_findings(
    config: &Config,
    graph: &GraphReport,
    metrics: &crate::model::MetricsReport,
) -> Result<Vec<PolicyFinding>> {
    let mut findings = Vec::new();
    let group_by_file = graph
        .files
        .iter()
        .map(|file| (file.path.clone(), file.group.clone()))
        .collect::<BTreeMap<_, _>>();

    findings.extend(evaluate_forbidden_dependencies(
        &config.policy.forbidden_dependencies,
        &group_by_file,
        graph,
    )?);
    findings.extend(evaluate_file_thresholds(&config.thresholds, &metrics.files));
    findings.extend(evaluate_function_thresholds(
        &config.thresholds,
        &metrics.functions,
    ));
    findings.extend(evaluate_cycles(
        &config.policy.allow_cycles,
        &group_by_file,
        graph,
    )?);

    findings.sort_by(|left, right| {
        severity_rank(&right.severity)
            .cmp(&severity_rank(&left.severity))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.rule_id.cmp(&right.rule_id))
            .then_with(|| left.message.cmp(&right.message))
    });

    Ok(findings)
}

fn evaluate_forbidden_dependencies(
    rules: &[ForbiddenDependencyRule],
    group_by_file: &BTreeMap<String, Option<String>>,
    graph: &GraphReport,
) -> Result<Vec<PolicyFinding>> {
    let mut findings = Vec::new();

    for edge in &graph.edges {
        let from_group = group_by_file
            .get(&edge.from)
            .and_then(|value| value.as_deref());
        let to_group = group_by_file
            .get(&edge.to)
            .and_then(|value| value.as_deref());

        for rule in rules {
            if from_group == Some(rule.from.as_str()) && to_group == Some(rule.to.as_str()) {
                let severity = Severity::parse(&rule.severity)?.to_string();
                findings.push(PolicyFinding {
                    rule_id: "forbidden_dependency".to_string(),
                    severity,
                    message: format!(
                        "{} depends on {} across forbidden boundary {} -> {}",
                        edge.from, edge.to, rule.from, rule.to
                    ),
                    file: Some(edge.from.clone()),
                    line: Some(edge.line),
                    related_files: vec![edge.from.clone(), edge.to.clone()],
                    suggested_fix: None,
                });
            }
        }
    }

    Ok(findings)
}

fn evaluate_file_thresholds(
    thresholds: &crate::config::ThresholdConfig,
    files: &[FileMetric],
) -> Vec<PolicyFinding> {
    let mut findings = Vec::new();

    for file in files {
        if let Some(severity) = threshold_severity(
            file.line_count,
            thresholds.file_lines_warn,
            thresholds.file_lines_critical,
        ) {
            findings.push(PolicyFinding {
                rule_id: "file_lines_threshold".to_string(),
                severity: severity.to_string(),
                message: format!(
                    "{} has {} lines (warn {}, critical {})",
                    file.file,
                    file.line_count,
                    thresholds.file_lines_warn,
                    thresholds.file_lines_critical
                ),
                file: Some(file.file.clone()),
                line: None,
                related_files: vec![file.file.clone()],
                suggested_fix: None,
            });
        }
    }

    findings
}

fn evaluate_function_thresholds(
    thresholds: &crate::config::ThresholdConfig,
    functions: &[FunctionMetric],
) -> Vec<PolicyFinding> {
    let mut findings = Vec::new();

    for function in functions {
        if let Some(severity) = threshold_severity(
            function.line_count,
            thresholds.function_lines_warn,
            thresholds.function_lines_critical,
        ) {
            findings.push(PolicyFinding {
                rule_id: "function_lines_threshold".to_string(),
                severity: severity.to_string(),
                message: format!(
                    "{}::{} spans {} lines (warn {}, critical {})",
                    function.file,
                    function.function_name,
                    function.line_count,
                    thresholds.function_lines_warn,
                    thresholds.function_lines_critical
                ),
                file: Some(function.file.clone()),
                line: Some(function.start_line),
                related_files: vec![function.file.clone()],
                suggested_fix: None,
            });
        }

        if let Some(severity) = threshold_severity(
            function.cyclomatic_estimate,
            thresholds.cyclomatic_warn,
            thresholds.cyclomatic_critical,
        ) {
            findings.push(PolicyFinding {
                rule_id: "cyclomatic_threshold".to_string(),
                severity: severity.to_string(),
                message: format!(
                    "{}::{} has cyclomatic estimate {} (warn {}, critical {})",
                    function.file,
                    function.function_name,
                    function.cyclomatic_estimate,
                    thresholds.cyclomatic_warn,
                    thresholds.cyclomatic_critical
                ),
                file: Some(function.file.clone()),
                line: Some(function.start_line),
                related_files: vec![function.file.clone()],
                suggested_fix: None,
            });
        }

        if let Some(severity) = threshold_severity(
            function.max_nesting_depth,
            thresholds.nesting_warn,
            thresholds.nesting_critical,
        ) {
            findings.push(PolicyFinding {
                rule_id: "nesting_threshold".to_string(),
                severity: severity.to_string(),
                message: format!(
                    "{}::{} has nesting depth {} (warn {}, critical {})",
                    function.file,
                    function.function_name,
                    function.max_nesting_depth,
                    thresholds.nesting_warn,
                    thresholds.nesting_critical
                ),
                file: Some(function.file.clone()),
                line: Some(function.start_line),
                related_files: vec![function.file.clone()],
                suggested_fix: None,
            });
        }

        if function.parameter_count >= thresholds.parameters_warn {
            findings.push(PolicyFinding {
                rule_id: "parameters_threshold".to_string(),
                severity: Severity::Warning.to_string(),
                message: format!(
                    "{}::{} has {} parameters (warn {})",
                    function.file,
                    function.function_name,
                    function.parameter_count,
                    thresholds.parameters_warn
                ),
                file: Some(function.file.clone()),
                line: Some(function.start_line),
                related_files: vec![function.file.clone()],
                suggested_fix: None,
            });
        }
    }

    findings
}

fn evaluate_cycles(
    allow_cycles: &[String],
    group_by_file: &BTreeMap<String, Option<String>>,
    graph: &GraphReport,
) -> Result<Vec<PolicyFinding>> {
    let allowlist = compile_allowlist(allow_cycles)?;
    let mut findings = Vec::new();

    for cycle in &graph.cycles {
        if is_allowlisted_cycle(cycle, &allowlist) {
            continue;
        }

        let severity = if cycle.iter().any(|file| {
            matches!(
                group_by_file.get(file).and_then(|group| group.as_deref()),
                Some("net") | Some("server") | Some("ai")
            )
        }) {
            Severity::Warning
        } else {
            Severity::Info
        };

        findings.push(PolicyFinding {
            rule_id: "cycle_detected".to_string(),
            severity: severity.to_string(),
            message: format!("dependency cycle detected: {}", cycle.join(" -> ")),
            file: cycle.first().cloned(),
            line: None,
            related_files: cycle.clone(),
            suggested_fix: None,
        });
    }

    Ok(findings)
}

fn compile_allowlist(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|error| GdHealthError::InvalidGlob {
            pattern: pattern.clone(),
            message: error.to_string(),
        })?;
        builder.add(glob);
    }

    let set = builder
        .build()
        .map_err(|error| GdHealthError::InvalidGlob {
            pattern: patterns.join(", "),
            message: error.to_string(),
        })?;

    Ok(Some(set))
}

fn is_allowlisted_cycle(cycle: &[String], allowlist: &Option<GlobSet>) -> bool {
    let Some(allowlist) = allowlist else {
        return false;
    };
    cycle.iter().all(|file| allowlist.is_match(file))
}

fn threshold_severity(value: usize, warn: usize, critical: usize) -> Option<Severity> {
    if value >= critical {
        Some(Severity::Critical)
    } else if value >= warn {
        Some(Severity::Warning)
    } else {
        None
    }
}

fn severity_rank(value: &str) -> usize {
    Severity::parse(value)
        .map(|severity| severity.rank())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "info" => Ok(Self::Info),
            "warning" | "warn" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            other => Err(GdHealthError::InvalidSeverity {
                severity: other.to_string(),
            }),
        }
    }

    fn rank(self) -> usize {
        match self {
            Self::Info => 0,
            Self::Warning => 1,
            Self::Critical => 2,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => f.write_str("info"),
            Self::Warning => f.write_str("warning"),
            Self::Critical => f.write_str("critical"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_findings, enforce_fail_on};
    use crate::config::{Config, ForbiddenDependencyRule};
    use crate::model::{
        DependencyEdge, DependencyKind, FileMetric, FunctionMetric, GraphReport, MetricsReport,
        OutputMetadata, PolicyFinding, ProjectFile,
    };

    #[test]
    fn emits_forbidden_dependency_and_threshold_findings() {
        let mut config = Config::default();
        config.policy.forbidden_dependencies = vec![ForbiddenDependencyRule {
            from: "server".to_string(),
            to: "hud".to_string(),
            severity: "critical".to_string(),
        }];
        config.policy.allow_cycles = vec![];

        let graph = GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: 2,
            files: vec![
                ProjectFile {
                    id: "server/a.gd".to_string(),
                    path: "server/a.gd".to_string(),
                    group: Some("server".to_string()),
                    line_count: 50,
                },
                ProjectFile {
                    id: "levels/main_level/hud/b.gd".to_string(),
                    path: "levels/main_level/hud/b.gd".to_string(),
                    group: Some("hud".to_string()),
                    line_count: 60,
                },
            ],
            edges: vec![DependencyEdge {
                from: "server/a.gd".to_string(),
                to: "levels/main_level/hud/b.gd".to_string(),
                kind: DependencyKind::ConstPreload,
                line: 12,
            }],
            cycles: vec![vec!["server/a.gd".to_string()]],
            parser_warnings: vec![],
            file_communities: Vec::new(),
            file_graph_metrics: None,
            entity_scan: None,
        };

        let metrics = MetricsReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            files: vec![FileMetric {
                file: "server/a.gd".to_string(),
                line_count: 1200,
                function_count: 1,
                number_of_methods: 1,
                signal_count: 0,
                public_function_count: 1,
                outgoing_dependency_count: 1,
                incoming_dependency_count: 0,
                cycle_count: 1,
                ws_complexity: 16,
                tfidf: 0.8,
                louvain_modularity: 0,
                file_hotspot_score: 10,
            }],
            functions: vec![FunctionMetric {
                file: "server/a.gd".to_string(),
                function_name: "oversized".to_string(),
                start_line: 20,
                end_line: 140,
                line_count: 121,
                branch_count: 10,
                cyclomatic_estimate: 16,
                max_nesting_depth: 7,
                parameter_count: 6,
                hotspot_score: 99,
            }],
            entities: Vec::new(),
            duplication: crate::model::DuplicationReport {
                available: true,
                unavailable_reason: None,
                clones: Vec::new(),
            },
        };

        let findings = collect_findings(&config, &graph, &metrics).expect("policy findings");

        assert!(
            findings
                .iter()
                .any(|finding| finding.rule_id == "forbidden_dependency")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule_id == "file_lines_threshold")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule_id == "function_lines_threshold")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule_id == "cyclomatic_threshold")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule_id == "nesting_threshold")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule_id == "parameters_threshold")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule_id == "cycle_detected")
        );
    }

    #[test]
    fn fail_on_blocks_when_threshold_is_met() {
        let findings = vec![
            PolicyFinding {
                rule_id: "x".to_string(),
                severity: "warning".to_string(),
                message: "warn".to_string(),
                file: None,
                line: None,
                related_files: vec![],
                suggested_fix: None,
            },
            PolicyFinding {
                rule_id: "y".to_string(),
                severity: "critical".to_string(),
                message: "crit".to_string(),
                file: None,
                line: None,
                related_files: vec![],
                suggested_fix: None,
            },
        ];

        assert!(enforce_fail_on(&findings, Some("critical")).is_err());
        assert!(enforce_fail_on(&findings, Some("warning")).is_err());
        assert!(enforce_fail_on(&findings, Some("info")).is_err());
        assert!(enforce_fail_on(&findings, None).is_ok());
    }
}
