use crate::analysis::{
    analyze_file_graph, build_entity_metrics, build_entity_scan, compute_tfidf_scores,
};
use crate::cli::CommandOptions;
use crate::config::Config;
use crate::duplication;
use crate::error::{GdHealthError, Result};
use crate::graph::build_graph;
use crate::isolate::isolate;
use crate::model::{FileMetric, FunctionMetric, MetricsReport, OutputMetadata};
use crate::model::{SignalFileFlow, SignalFlowReport};
use crate::parser::signal_flow::analyze_signal_flow;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

static SIGNAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*signal\s+([A-Za-z_][A-Za-z0-9_]*)").expect("valid signal regex")
});

static FUNC_START_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:static\s+)?func\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(").expect("valid func regex")
});

static CONTROL_FLOW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(if|elif|else|for|while|match)\b").expect("valid control-flow regex")
});

static CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)").expect("valid class regex")
});

static PROPERTY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:@\w+(?:\([^)]*\))?\s+)*(?:var|const)\s+([A-Za-z_][A-Za-z0-9_]*)")
        .expect("valid property regex")
});

pub fn compute_metrics(config: &Config, options: &CommandOptions) -> Result<MetricsReport> {
    let graph = build_graph(config, options)?;
    let root = options.root.as_path();
    let file_graph_analysis = analyze_file_graph(&graph);
    let tfidf_scores = compute_tfidf_scores(root, &graph)?;
    let community_by_file = file_graph_analysis
        .communities
        .iter()
        .map(|item| (item.id.as_str(), item.community_id))
        .collect::<BTreeMap<_, _>>();

    let incoming_counts = build_incoming_counts(&graph.edges);
    let cycle_memberships = build_cycle_memberships(&graph.cycles);

    let mut file_metrics = Vec::new();
    let mut function_metrics = Vec::new();
    let mut signal_files = Vec::new();
    let mut unresolved_signal_references = Vec::new();

    for file in &graph.files {
        let content = read_file(root, &file.path)?;
        let analysis = analyze_file(&file.path, &content);
        let signal_flow = analyze_signal_flow(&file.path, &content);
        let outgoing_dependency_count = graph
            .edges
            .iter()
            .filter(|edge| edge.from == file.path)
            .count();
        let incoming_dependency_count = incoming_counts.get(&file.path).copied().unwrap_or(0);
        let cycle_count = cycle_memberships.get(&file.path).copied().unwrap_or(0);

        let mut file_function_metrics = analysis
            .functions
            .into_iter()
            .map(|function| FunctionMetric {
                file: file.path.clone(),
                function_name: function.function_name,
                start_line: function.start_line,
                end_line: function.end_line,
                line_count: function.line_count,
                branch_count: function.branch_count,
                cyclomatic_estimate: function.cyclomatic_estimate,
                max_nesting_depth: function.max_nesting_depth,
                parameter_count: function.parameter_count,
                hotspot_score: compute_function_hotspot_score(
                    function.cyclomatic_estimate,
                    function.max_nesting_depth,
                    function.parameter_count,
                    function.line_count,
                ),
            })
            .collect::<Vec<_>>();

        file_function_metrics.sort_by(|left, right| {
            right
                .hotspot_score
                .cmp(&left.hotspot_score)
                .then_with(|| left.file.cmp(&right.file))
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.function_name.cmp(&right.function_name))
        });

        let file_hotspot_score = compute_file_hotspot_score(
            &file_function_metrics,
            incoming_dependency_count,
            outgoing_dependency_count,
            cycle_count,
        );
        let ws_complexity = file_function_metrics
            .iter()
            .map(|function| function.cyclomatic_estimate * (function.max_nesting_depth + 1))
            .sum::<usize>();

        file_metrics.push(FileMetric {
            file: file.path.clone(),
            line_count: file.line_count,
            function_count: file_function_metrics.len(),
            number_of_methods: file_function_metrics.len(),
            signal_count: signal_flow.declarations.len(),
            signal_emit_count: signal_flow.emit_count,
            signal_connection_count: signal_flow.connection_count,
            unresolved_signal_reference_count: signal_flow.unresolved_references.len(),
            public_function_count: file_function_metrics
                .iter()
                .filter(|function| !function.function_name.starts_with('_'))
                .count(),
            outgoing_dependency_count,
            incoming_dependency_count,
            cycle_count,
            ws_complexity,
            tfidf: tfidf_scores.get(&file.path).copied().unwrap_or(0.0),
            louvain_modularity: community_by_file
                .get(file.path.as_str())
                .copied()
                .unwrap_or(0),
            file_hotspot_score,
        });

        function_metrics.extend(file_function_metrics);
        unresolved_signal_references.extend(signal_flow.unresolved_references);
        signal_files.push(SignalFileFlow {
            file: file.path.clone(),
            declarations: signal_flow.declarations,
            signal_emit_count: signal_flow.emit_count,
            signal_connection_count: signal_flow.connection_count,
            unresolved_signal_reference_count: file_metrics
                .last()
                .expect("file metric just added")
                .unresolved_signal_reference_count,
        });
    }

    file_metrics.sort_by(|left, right| {
        right
            .file_hotspot_score
            .cmp(&left.file_hotspot_score)
            .then_with(|| right.line_count.cmp(&left.line_count))
            .then_with(|| left.file.cmp(&right.file))
    });

    function_metrics.sort_by(|left, right| {
        right
            .hotspot_score
            .cmp(&left.hotspot_score)
            .then_with(|| right.cyclomatic_estimate.cmp(&left.cyclomatic_estimate))
            .then_with(|| right.line_count.cmp(&left.line_count))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.function_name.cmp(&right.function_name))
    });
    signal_files.sort_by(|left, right| left.file.cmp(&right.file));
    unresolved_signal_references.sort_by(|left, right| {
        (
            &left.file,
            left.line,
            &left.operation,
            &left.signal,
            &left.receiver,
        )
            .cmp(&(
                &right.file,
                right.line,
                &right.operation,
                &right.signal,
                &right.receiver,
            ))
    });

    let entity_scan = build_entity_scan(root, &graph, &file_metrics)?;
    let entity_metrics = build_entity_metrics(&entity_scan, &file_metrics);

    let duplication_config = config.duplication.clone();
    let duplication_report = isolate("duplication", || {
        duplication::detect(root, &graph, &duplication_config)
    });

    Ok(MetricsReport {
        metadata: OutputMetadata::default(),
        root: options.root.to_string_lossy().into_owned(),
        files: file_metrics,
        functions: function_metrics,
        entities: entity_metrics,
        signal_flow: SignalFlowReport {
            available: true,
            files: signal_files,
            unresolved_references: unresolved_signal_references,
        },
        duplication: duplication_report,
    })
}

fn read_file(root: &Path, path: &str) -> Result<String> {
    let absolute_path = root.join(path);
    fs::read_to_string(&absolute_path).map_err(|source| GdHealthError::ReadFile {
        path: absolute_path,
        source,
    })
}

fn build_incoming_counts(edges: &[crate::model::DependencyEdge]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for edge in edges {
        *counts.entry(edge.to.clone()).or_insert(0) += 1;
    }
    counts
}

fn build_cycle_memberships(cycles: &[Vec<String>]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for cycle in cycles {
        for file in cycle {
            *counts.entry(file.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn compute_function_hotspot_score(
    cyclomatic_estimate: usize,
    max_nesting_depth: usize,
    parameter_count: usize,
    line_count: usize,
) -> usize {
    cyclomatic_estimate * 3
        + max_nesting_depth * 2
        + parameter_count
        + line_count.saturating_sub(20) / 10
}

fn compute_file_hotspot_score(
    functions: &[FunctionMetric],
    incoming_dependency_count: usize,
    outgoing_dependency_count: usize,
    cycle_count: usize,
) -> usize {
    functions
        .iter()
        .take(5)
        .map(|function| function.hotspot_score)
        .sum::<usize>()
        + incoming_dependency_count * 2
        + outgoing_dependency_count
        + cycle_count * 5
}

#[derive(Debug)]
pub(crate) struct FileAnalysis {
    #[allow(dead_code)] // retained for direct metric-analysis tests and symbol tooling
    signal_count: usize,
    pub(crate) functions: Vec<FunctionAnalysis>,
}

#[derive(Debug)]
pub(crate) struct FunctionAnalysis {
    function_name: String,
    start_line: usize,
    end_line: usize,
    line_count: usize,
    branch_count: usize,
    cyclomatic_estimate: usize,
    max_nesting_depth: usize,
    parameter_count: usize,
}

pub(crate) fn analyze_file(_path: &str, content: &str) -> FileAnalysis {
    let lines = content.lines().collect::<Vec<_>>();
    let signal_count = lines
        .iter()
        .filter(|line| SIGNAL_RE.is_match(strip_comments(line)))
        .count();

    let function_headers = find_function_headers(&lines);
    let mut functions = Vec::new();

    for index in 0..function_headers.len() {
        let header = &function_headers[index];
        let end_line = determine_function_end_line(index, &function_headers, lines.len());
        let branch_count =
            compute_branch_count(&lines[header.start_line.saturating_sub(1)..end_line]);
        let max_nesting_depth =
            compute_max_nesting_depth(&lines[header.body_start_line.saturating_sub(1)..end_line]);
        let parameter_count = count_parameters(&header.signature_text);
        let line_count = end_line.saturating_sub(header.start_line) + 1;

        functions.push(FunctionAnalysis {
            function_name: header.function_name.clone(),
            start_line: header.start_line,
            end_line,
            line_count,
            branch_count,
            cyclomatic_estimate: 1 + branch_count,
            max_nesting_depth,
            parameter_count,
        });
    }

    FileAnalysis {
        signal_count,
        functions,
    }
}

#[derive(Debug)]
struct FunctionHeader {
    function_name: String,
    start_line: usize,
    body_start_line: usize,
    signature_text: String,
}

fn find_function_headers(lines: &[&str]) -> Vec<FunctionHeader> {
    let mut headers = Vec::new();
    let mut line_index = 0;

    while line_index < lines.len() {
        let line = lines[line_index];
        let code = strip_comments(line);
        if let Some(captures) = FUNC_START_RE.captures(code) {
            let start_line = line_index + 1;
            let function_name = captures
                .get(1)
                .map(|name| name.as_str().to_string())
                .unwrap_or_else(|| "<anonymous>".to_string());

            let mut signature_parts = vec![code.trim_end().to_string()];
            let mut body_start_line = start_line;
            let mut signature_index = line_index;

            while !strip_comments(lines[signature_index]).contains(':')
                && signature_index + 1 < lines.len()
            {
                signature_index += 1;
                signature_parts.push(
                    strip_comments(lines[signature_index])
                        .trim_end()
                        .to_string(),
                );
                body_start_line = signature_index + 1;
            }

            headers.push(FunctionHeader {
                function_name,
                start_line,
                body_start_line,
                signature_text: signature_parts.join(" "),
            });

            line_index = signature_index + 1;
            continue;
        }

        line_index += 1;
    }

    headers
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolKind {
    Function,
    InnerClass,
    Signal,
    Property,
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolSpan {
    #[allow(dead_code)] // read by tests and reserved for future weighting
    pub kind: SymbolKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Top-level (indentation-0) symbol spans in a file: functions, inner
/// classes, signals, and var/const properties — the substrate for the
/// Split File refactoring suggestion. Reuses the same header-detection and
/// end-line inference as `find_function_headers`, generalized from one
/// symbol kind to four.
pub(crate) fn find_top_level_symbol_spans(lines: &[&str]) -> Vec<SymbolSpan> {
    struct RawHeader {
        kind: SymbolKind,
        name: String,
        start_line: usize,
    }

    let mut headers = Vec::<RawHeader>::new();

    for (index, line) in lines.iter().enumerate() {
        if indentation_width(line) != 0 {
            continue;
        }
        let code = strip_comments(line);
        let start_line = index + 1;

        if let Some(captures) = FUNC_START_RE.captures(code) {
            headers.push(RawHeader {
                kind: SymbolKind::Function,
                name: captures
                    .get(1)
                    .map_or_else(String::new, |m| m.as_str().to_string()),
                start_line,
            });
        } else if let Some(captures) = CLASS_RE.captures(code) {
            headers.push(RawHeader {
                kind: SymbolKind::InnerClass,
                name: captures
                    .get(1)
                    .map_or_else(String::new, |m| m.as_str().to_string()),
                start_line,
            });
        } else if let Some(captures) = SIGNAL_RE.captures(code) {
            headers.push(RawHeader {
                kind: SymbolKind::Signal,
                name: captures
                    .get(1)
                    .map_or_else(String::new, |m| m.as_str().to_string()),
                start_line,
            });
        } else if let Some(captures) = PROPERTY_RE.captures(code) {
            headers.push(RawHeader {
                kind: SymbolKind::Property,
                name: captures
                    .get(1)
                    .map_or_else(String::new, |m| m.as_str().to_string()),
                start_line,
            });
        }
    }

    let total_lines = lines.len();
    headers
        .iter()
        .enumerate()
        .filter(|(_, header)| !header.name.is_empty())
        .map(|(index, header)| {
            let end_line = headers
                .get(index + 1)
                .map(|next| next.start_line.saturating_sub(1))
                .unwrap_or(total_lines);
            SymbolSpan {
                kind: header.kind,
                name: header.name.clone(),
                start_line: header.start_line,
                end_line,
            }
        })
        .collect()
}

fn determine_function_end_line(
    index: usize,
    headers: &[FunctionHeader],
    total_lines: usize,
) -> usize {
    match headers.get(index + 1) {
        Some(next) => next.start_line.saturating_sub(1),
        None => total_lines,
    }
}

fn compute_branch_count(lines: &[&str]) -> usize {
    lines
        .iter()
        .filter(|line| CONTROL_FLOW_RE.is_match(strip_comments(line)))
        .count()
}

fn compute_max_nesting_depth(lines: &[&str]) -> usize {
    let mut stack = Vec::<usize>::new();
    let mut max_depth = 0;

    for line in lines {
        let code = strip_comments(line).trim_end();
        if code.trim().is_empty() {
            continue;
        }

        let indent = indentation_width(line);
        while let Some(last_indent) = stack.last().copied() {
            if indent <= last_indent {
                stack.pop();
            } else {
                break;
            }
        }

        if CONTROL_FLOW_RE.is_match(code) && code.ends_with(':') {
            stack.push(indent);
            max_depth = max_depth.max(stack.len());
        }
    }

    max_depth
}

fn count_parameters(signature_text: &str) -> usize {
    let Some(open_index) = signature_text.find('(') else {
        return 0;
    };
    let Some(close_index) = find_matching_paren(signature_text, open_index) else {
        return 0;
    };
    let params = &signature_text[open_index + 1..close_index];
    split_top_level_csv(params).len()
}

fn find_matching_paren(text: &str, open_index: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in text.char_indices().skip(open_index) {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_csv(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth_paren = 0usize;
    let mut depth_bracket = 0usize;
    let mut depth_brace = 0usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    for ch in text.chars() {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                current.push(ch);
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                current.push(ch);
            }
            '(' if !in_single_quote && !in_double_quote => {
                depth_paren += 1;
                current.push(ch);
            }
            ')' if !in_single_quote && !in_double_quote => {
                depth_paren = depth_paren.saturating_sub(1);
                current.push(ch);
            }
            '[' if !in_single_quote && !in_double_quote => {
                depth_bracket += 1;
                current.push(ch);
            }
            ']' if !in_single_quote && !in_double_quote => {
                depth_bracket = depth_bracket.saturating_sub(1);
                current.push(ch);
            }
            '{' if !in_single_quote && !in_double_quote => {
                depth_brace += 1;
                current.push(ch);
            }
            '}' if !in_single_quote && !in_double_quote => {
                depth_brace = depth_brace.saturating_sub(1);
                current.push(ch);
            }
            ',' if !in_single_quote
                && !in_double_quote
                && depth_paren == 0
                && depth_bracket == 0
                && depth_brace == 0 =>
            {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    let tail = current.trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }

    parts
}

fn indentation_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

fn strip_comments(line: &str) -> &str {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '#' if !in_single_quote && !in_double_quote => return &line[..index],
            _ => {}
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::{
        SymbolKind, analyze_file, compute_file_hotspot_score, compute_function_hotspot_score,
        count_parameters, find_top_level_symbol_spans,
    };
    use crate::model::FunctionMetric;

    #[test]
    fn counts_multiline_parameters() {
        let signature = "static func death( eid: int, t: int, ticks_until_respawn: int, killer: String, victim: String ) -> GameCommand:";
        assert_eq!(count_parameters(signature), 5);
    }

    #[test]
    fn computes_function_metrics_from_indented_blocks() {
        let analysis = analyze_file(
            "sample.gd",
            r#"
signal hp_changed(current: int, max_hp: int)

func _ready() -> void:
    if true:
        for i in range(3):
            print(i)
    else:
        print("fallback")

static func helper(a: int, b: int = 2) -> int:
    return a + b
"#,
        );

        assert_eq!(analysis.signal_count, 1);
        assert_eq!(analysis.functions.len(), 2);
        assert_eq!(analysis.functions[0].branch_count, 3);
        assert_eq!(analysis.functions[0].max_nesting_depth, 2);
        assert_eq!(analysis.functions[1].parameter_count, 2);
    }

    #[test]
    fn computes_hotspot_scores_deterministically() {
        assert_eq!(compute_function_hotspot_score(7, 3, 4, 55), 34);

        let functions = vec![
            FunctionMetric {
                file: "a.gd".to_string(),
                function_name: "alpha".to_string(),
                start_line: 1,
                end_line: 20,
                line_count: 20,
                branch_count: 4,
                cyclomatic_estimate: 5,
                max_nesting_depth: 2,
                parameter_count: 2,
                hotspot_score: 21,
            },
            FunctionMetric {
                file: "a.gd".to_string(),
                function_name: "beta".to_string(),
                start_line: 21,
                end_line: 60,
                line_count: 40,
                branch_count: 5,
                cyclomatic_estimate: 6,
                max_nesting_depth: 3,
                parameter_count: 1,
                hotspot_score: 28,
            },
        ];

        assert_eq!(compute_file_hotspot_score(&functions, 2, 3, 1), 61);
    }

    #[test]
    fn finds_top_level_symbol_spans_across_kinds_and_skips_nested_lines() {
        let content = r#"extends RefCounted

signal hp_changed(current: int)

@export var max_hp: int = 100

func _ready() -> void:
    if true:
        var local_only = 1
        print(local_only)

class Inner:
    var nested_field = 1
"#;
        let lines = content.lines().collect::<Vec<_>>();
        let spans = find_top_level_symbol_spans(&lines);
        let kinds = spans
            .iter()
            .map(|s| (s.kind, s.name.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                (SymbolKind::Signal, "hp_changed"),
                (SymbolKind::Property, "max_hp"),
                (SymbolKind::Function, "_ready"),
                (SymbolKind::InnerClass, "Inner"),
            ]
        );
        // `local_only` (indented, inside _ready) must not appear as a top-level span.
        assert!(!spans.iter().any(|s| s.name == "local_only"));
    }
}
