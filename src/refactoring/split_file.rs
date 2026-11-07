//! Split File: for an oversized file, cluster its top-level symbols by a
//! weighted cohesion graph (call references + shared preload/load/extends
//! targets), then reuse the existing Louvain community detection
//! (`analysis::detect_communities`, already used for the file-level graph)
//! to see if a clean, non-trivial split exists. Only emits a suggestion
//! when the partition clears a modularity gate — no suggestion is better
//! than a bad one.

use crate::analysis::{compute_modularity_for_ids, detect_communities};
use crate::config::{RefactoringConfig, ThresholdConfig};
use crate::metrics::find_top_level_symbol_spans;
use crate::model::{GraphReport, MetricsReport, PolicyFinding, RefactorSuggestion};
use crate::parser::extract::parse_file;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

static IDENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").expect("valid identifier regex"));

// A "split into N groups" suggestion stops being actionable once N gets
// large — Louvain on a sparse, mostly-locally-connected symbol graph tends
// to fragment into many small communities rather than a few big cohesive
// ones. Past this many groups, it's more likely over-fragmentation noise
// than a genuine refactor plan, so no suggestion beats a 13-way split.
const MAX_PROPOSED_GROUPS: usize = 6;

pub fn detect(
    root: &Path,
    graph: &GraphReport,
    metrics: &MetricsReport,
    thresholds: &ThresholdConfig,
    config: &RefactoringConfig,
) -> Vec<PolicyFinding> {
    let mut findings = Vec::new();

    for file_metric in &metrics.files {
        if file_metric.line_count <= thresholds.file_lines_warn {
            continue;
        }
        let Some(finding) = detect_for_file(root, graph, file_metric, config) else {
            continue;
        };
        findings.push(finding);
    }

    findings
}

fn detect_for_file(
    root: &Path,
    graph: &GraphReport,
    file_metric: &crate::model::FileMetric,
    config: &RefactoringConfig,
) -> Option<PolicyFinding> {
    let path = &file_metric.file;
    let absolute_path = root.join(path);
    let content = std::fs::read_to_string(&absolute_path).ok()?;
    let lines = content.lines().collect::<Vec<_>>();

    let spans = find_top_level_symbol_spans(&lines);
    if spans.len() < config.split_file_min_symbols {
        return None;
    }

    let weighted_pairs = build_cohesion_pairs(&spans, &lines, &content, path);
    if weighted_pairs.is_empty() {
        return None;
    }

    let symbol_ids = spans
        .iter()
        .map(|span| span.name.clone())
        .collect::<Vec<_>>();
    let communities = detect_communities(&symbol_ids, &weighted_pairs);
    let modularity = compute_modularity_for_ids(&symbol_ids, &weighted_pairs, &communities);

    let mut groups = BTreeMap::<usize, Vec<String>>::new();
    for community in &communities {
        groups
            .entry(community.community_id)
            .or_default()
            .push(community.id.clone());
    }

    if groups.len() < 2
        || groups.len() > MAX_PROPOSED_GROUPS
        || modularity < config.split_file_modularity_gate
    {
        return None;
    }
    if groups.values().any(|group| group.len() < 2) {
        // A singleton "leftover" group means the split isn't clean —
        // better to suggest nothing than a lopsided partition.
        return None;
    }

    let proposed_groups = groups
        .into_values()
        .map(|mut group| {
            group.sort();
            group
        })
        .collect::<Vec<_>>();
    let _ = graph; // reserved for a future cross-file blast-radius check

    Some(PolicyFinding {
        rule_id: "split-file-candidate".to_string(),
        severity: "info".to_string(),
        message: format!(
            "{path} ({} lines) could split into {} cohesive groups (modularity {modularity:.2})",
            file_metric.line_count,
            proposed_groups.len()
        ),
        file: Some(path.clone()),
        line: None,
        related_files: vec![path.clone()],
        suggested_fix: Some(RefactorSuggestion::SplitFile {
            proposed_groups,
            modularity,
        }),
    })
}

/// Two signals, both graph-only (no git needed):
/// - call-reference: how often span A's body mentions span B's name.
/// - shared-import-surface: spans A and B both reference the same
///   preload/load/extends target within their line ranges.
fn build_cohesion_pairs(
    spans: &[crate::metrics::SymbolSpan],
    lines: &[&str],
    content: &str,
    path: &str,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();

    for i in 0..spans.len() {
        let body = span_text(spans[i].start_line, spans[i].end_line, lines);
        let identifier_counts = count_identifiers(&body);
        for j in 0..spans.len() {
            if i == j {
                continue;
            }
            let count = identifier_counts
                .get(spans[j].name.as_str())
                .copied()
                .unwrap_or(0);
            for _ in 0..count {
                pairs.push((spans[i].name.clone(), spans[j].name.clone()));
            }
        }
    }

    let parsed = parse_file(path, content);
    let mut targets_by_span = vec![BTreeSet::<String>::new(); spans.len()];
    for dependency in &parsed.dependencies {
        let target_key = format!("{:?}", dependency.target);
        for (index, span) in spans.iter().enumerate() {
            if dependency.line >= span.start_line && dependency.line <= span.end_line {
                targets_by_span[index].insert(target_key.clone());
            }
        }
    }
    for i in 0..spans.len() {
        for j in (i + 1)..spans.len() {
            let shared = targets_by_span[i].intersection(&targets_by_span[j]).count();
            for _ in 0..shared {
                pairs.push((spans[i].name.clone(), spans[j].name.clone()));
            }
        }
    }

    pairs
}

fn span_text(start_line: usize, end_line: usize, lines: &[&str]) -> String {
    lines
        .get(start_line.saturating_sub(1)..end_line.min(lines.len()))
        .unwrap_or(&[])
        .join("\n")
}

fn count_identifiers(text: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for identifier in IDENT_RE.find_iter(text) {
        *counts.entry(identifier.as_str().to_string()).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::detect;
    use crate::config::{RefactoringConfig, ThresholdConfig};
    use crate::model::{FileMetric, GraphReport, MetricsReport, OutputMetadata};

    fn write_fixture(root: &std::path::Path, path: &str, content: &str) {
        std::fs::create_dir_all(root).expect("create root");
        std::fs::write(root.join(path), content).expect("write fixture");
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "gdhealth-split-file-{prefix}-{}",
            std::process::id()
        ))
    }

    fn base_metrics(file: &str, line_count: usize) -> MetricsReport {
        MetricsReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            files: vec![FileMetric {
                file: file.to_string(),
                line_count,
                function_count: 0,
                number_of_methods: 0,
                signal_count: 0,
                signal_emit_count: 0,
                signal_connection_count: 0,
                unresolved_signal_reference_count: 0,
                public_function_count: 0,
                outgoing_dependency_count: 0,
                incoming_dependency_count: 0,
                cycle_count: 0,
                ws_complexity: 0,
                tfidf: 0.0,
                louvain_modularity: 0,
                file_hotspot_score: 0,
            }],
            functions: vec![],
            entities: vec![],
            signal_flow: crate::model::SignalFlowReport {
                available: true,
                files: vec![],
                unresolved_references: vec![],
            },
            duplication: crate::model::DuplicationReport {
                available: true,
                unavailable_reason: None,
                clones: vec![],
            },
        }
    }

    fn empty_graph() -> GraphReport {
        GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: 0,
            files: vec![],
            edges: vec![],
            cycles: vec![],
            parser_warnings: vec![],
            file_communities: vec![],
            file_graph_metrics: None,
            entity_scan: None,
        }
    }

    #[test]
    fn suggests_a_split_for_two_clearly_separate_symbol_clusters() {
        let root = temp_dir("cohesive");
        // Two obviously separate feature areas stitched into one file:
        // inventory_* functions only call each other, shop_* functions
        // only call each other, no cross-talk.
        let content = r#"extends Node

func inventory_add(item) -> void:
    inventory_validate(item)

func inventory_validate(item) -> void:
    inventory_store(item)

func inventory_store(item) -> void:
    pass

func shop_buy(item) -> void:
    shop_charge(item)

func shop_charge(item) -> void:
    shop_log(item)

func shop_log(item) -> void:
    pass
"#;
        write_fixture(&root, "mixed.gd", content);
        let metrics = base_metrics("mixed.gd", 20);
        let thresholds = ThresholdConfig {
            file_lines_warn: 5,
            ..Default::default()
        };
        let config = RefactoringConfig {
            split_file_min_symbols: 4,
            split_file_modularity_gate: 0.1,
            ..Default::default()
        };

        let findings = detect(&root, &empty_graph(), &metrics, &thresholds, &config);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "split-file-candidate");
        match findings[0].suggested_fix.as_ref().expect("suggestion") {
            crate::model::RefactorSuggestion::SplitFile {
                proposed_groups, ..
            } => {
                assert_eq!(proposed_groups.len(), 2);
            }
            _ => panic!("expected SplitFile suggestion"),
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn does_not_suggest_a_split_below_the_file_lines_threshold() {
        let root = temp_dir("small");
        write_fixture(&root, "small.gd", "extends Node\nfunc a():\n    pass\n");
        let metrics = base_metrics("small.gd", 3);
        let thresholds = ThresholdConfig::default(); // warn = 500, well above 3
        let config = RefactoringConfig::default();

        let findings = detect(&root, &empty_graph(), &metrics, &thresholds, &config);
        assert!(findings.is_empty());

        std::fs::remove_dir_all(&root).ok();
    }
}
