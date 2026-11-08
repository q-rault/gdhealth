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
use crate::parser::signal_flow::analyze_signal_flow_tree;
use crate::parser::structure::{self, FunctionKind};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(crate) use crate::parser::structure::SymbolSpan;

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
        let tree = gdsyntax::parse(content);
        let analysis = analyze_tree(&tree);
        let signal_flow = analyze_signal_flow_tree(&file.path, &tree);
        let outgoing_dependency_count = graph
            .edges
            .iter()
            .filter(|edge| edge.from == file.path)
            .count();
        let incoming_dependency_count = incoming_counts.get(&file.path).copied().unwrap_or(0);
        let cycle_count = cycle_memberships.get(&file.path).copied().unwrap_or(0);

        let method_names = analysis
            .functions
            .iter()
            .filter(|function| function.kind == FunctionKind::Func)
            .map(|function| function.function_name.clone())
            .collect::<Vec<_>>();
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
            function_count: method_names.len(),
            number_of_methods: method_names.len(),
            signal_count: signal_flow.declarations.len(),
            signal_emit_count: signal_flow.emit_count,
            signal_connection_count: signal_flow.connection_count,
            unresolved_signal_reference_count: signal_flow.unresolved_references.len(),
            public_function_count: method_names
                .iter()
                .filter(|name| !name.starts_with('_'))
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
    /// `func` declarations and property accessors with a body.
    pub(crate) functions: Vec<FunctionAnalysis>,
}

impl FileAnalysis {
    /// `func` declarations, at file level and in inner classes; property
    /// accessors are measured but are not methods.
    pub(crate) fn method_count(&self) -> usize {
        self.functions
            .iter()
            .filter(|function| function.kind == FunctionKind::Func)
            .count()
    }
}

#[derive(Debug)]
pub(crate) struct FunctionAnalysis {
    kind: FunctionKind,
    function_name: String,
    start_line: usize,
    end_line: usize,
    line_count: usize,
    branch_count: usize,
    cyclomatic_estimate: usize,
    max_nesting_depth: usize,
    parameter_count: usize,
}

#[cfg(test)]
pub(crate) fn analyze_file(_path: &str, content: &str) -> FileAnalysis {
    analyze_tree(&gdsyntax::parse(content))
}

pub(crate) fn analyze_tree(tree: &gdsyntax::SyntaxTree) -> FileAnalysis {
    let functions = structure::functions(tree)
        .into_iter()
        .map(|shape| FunctionAnalysis {
            kind: shape.kind,
            function_name: shape.name,
            start_line: shape.start_line,
            end_line: shape.end_line,
            line_count: shape.end_line - shape.start_line + 1,
            branch_count: shape.branch_count,
            cyclomatic_estimate: 1 + shape.branch_count,
            max_nesting_depth: shape.max_nesting_depth,
            parameter_count: shape.parameter_count,
        })
        .collect();
    FileAnalysis { functions }
}

/// File-level symbol spans: functions, inner classes, signals and
/// var/const properties — the substrate for the Split File refactoring
/// suggestion.
pub(crate) fn find_top_level_symbol_spans(tree: &gdsyntax::SyntaxTree) -> Vec<SymbolSpan> {
    structure::top_level_symbol_spans(tree)
}

#[cfg(test)]
mod tests {
    use super::{
        FileAnalysis, FunctionAnalysis, analyze_file, compute_file_hotspot_score,
        compute_function_hotspot_score, find_top_level_symbol_spans,
    };
    use crate::model::FunctionMetric;
    use crate::parser::structure::{FunctionKind, SymbolKind};

    fn function<'a>(analysis: &'a FileAnalysis, name: &str) -> &'a FunctionAnalysis {
        analysis
            .functions
            .iter()
            .find(|function| function.function_name == name)
            .unwrap_or_else(|| panic!("no function {name}"))
    }

    #[test]
    fn counts_multiline_parameters() {
        let analysis = analyze_file(
            "sample.gd",
            "static func death(\n\teid: int,\n\tt: int, ticks_until_respawn: int,\n\tkiller: String, victim: String\n) -> int:\n\treturn 0\n",
        );
        assert_eq!(analysis.functions[0].parameter_count, 5);
        assert_eq!(analysis.functions[0].start_line, 1);
        assert_eq!(analysis.functions[0].end_line, 6);
    }

    #[test]
    fn computes_function_metrics_from_the_tree() {
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

        assert_eq!(analysis.functions.len(), 2);
        // `if` and `for` are decisions; `else` is not.
        assert_eq!(analysis.functions[0].branch_count, 2);
        assert_eq!(analysis.functions[0].max_nesting_depth, 2);
        // The function ends on its last line of code, not at the next `func`.
        assert_eq!(analysis.functions[0].end_line, 9);
        assert_eq!(analysis.functions[1].parameter_count, 2);
    }

    #[test]
    fn counts_boolean_operators_ternaries_and_match_patterns() {
        let analysis = analyze_file(
            "sample.gd",
            r#"
func pick(kind, a, b):
    if a and b or not a:
        pass
    var x = 1 if a else 2
    match kind:
        1, 2:
            pass
        3 when a && b:
            pass
        var other:
            pass
        _:
            pass
"#,
        );
        let pick = function(&analysis, "pick");
        // if + and + or + ternary + patterns 1, 2, 3 + when + &&;
        // `var other` and `_` are catch-alls.
        assert_eq!(pick.branch_count, 9);
        assert_eq!(pick.cyclomatic_estimate, 10);
        assert_eq!(pick.max_nesting_depth, 1);
    }

    #[test]
    fn keeps_lambdas_in_their_function_and_reads_inner_classes() {
        let analysis = analyze_file(
            "sample.gd",
            r#"
func outer():
    var cb = func named_lambda(value):
        if value:
            return 1
        return 0
    return cb

class Inner:
    func inner_method(a, b, c):
        while a:
            a -= 1
"#,
        );
        let names = analysis
            .functions
            .iter()
            .map(|function| function.function_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["outer", "inner_method"]);
        let outer = function(&analysis, "outer");
        assert_eq!((outer.start_line, outer.end_line), (2, 7));
        assert_eq!(outer.branch_count, 1);
        assert_eq!(outer.max_nesting_depth, 1);
        let inner = function(&analysis, "inner_method");
        assert_eq!(inner.parameter_count, 3);
        assert_eq!(inner.branch_count, 1);
        assert_eq!(analysis.method_count(), 2);
    }

    #[test]
    fn measures_property_accessors_but_does_not_count_them_as_methods() {
        let analysis = analyze_file(
            "sample.gd",
            r#"
var hp: int = 0:
    set(value):
        if value < 0:
            value = 0
        hp = value
    get:
        return hp

func heal():
    hp += 1
"#,
        );
        let setter = function(&analysis, "hp.set");
        assert_eq!(setter.kind, FunctionKind::Accessor);
        assert_eq!((setter.start_line, setter.end_line), (3, 6));
        assert_eq!(setter.branch_count, 1);
        assert_eq!(setter.parameter_count, 1);
        assert_eq!(function(&analysis, "hp.get").line_count, 2);
        // The accessor's `if` does not leak into a neighbouring function.
        assert_eq!(function(&analysis, "heal").branch_count, 0);
        assert_eq!(analysis.method_count(), 1);
    }

    #[test]
    fn ignores_code_shaped_text_in_strings_and_comments() {
        let analysis = analyze_file(
            "sample.gd",
            "func describe():\n    var help = \"\"\"\nfunc not_a_function():\n    if true:\n        pass\n\"\"\"\n    # if this were code\n    return help\n",
        );
        assert_eq!(analysis.functions.len(), 1);
        assert_eq!(analysis.functions[0].branch_count, 0);
        assert_eq!(analysis.functions[0].end_line, 8);
    }

    #[test]
    fn inline_blocks_count_toward_nesting() {
        let analysis = analyze_file(
            "sample.gd",
            "func guard(x):\n    if not x: return\n    for i in x: if i: print(i)\n",
        );
        assert_eq!(analysis.functions[0].max_nesting_depth, 2);
        assert_eq!(analysis.functions[0].branch_count, 3);
    }

    #[test]
    fn measures_the_readable_part_of_a_file_with_syntax_errors() {
        let analysis = analyze_file(
            "broken.gd",
            "func ok(a):\n    if a:\n        pass\n\nfunc broken(:\n    pass\n\nfunc after():\n    while true:\n        break\n",
        );
        assert_eq!(function(&analysis, "ok").branch_count, 1);
        assert_eq!(function(&analysis, "after").branch_count, 1);
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
        let tree = gdsyntax::parse(content);
        let spans = find_top_level_symbol_spans(&tree);
        let kinds = spans
            .iter()
            .map(|s| (s.kind, s.name.as_str(), s.start_line, s.end_line))
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                (SymbolKind::Signal, "hp_changed", 3, 3),
                (SymbolKind::Property, "max_hp", 5, 5),
                (SymbolKind::Function, "_ready", 7, 10),
                (SymbolKind::InnerClass, "Inner", 12, 13),
            ]
        );
        // `local_only` (indented, inside _ready) must not appear as a top-level span.
        assert!(!spans.iter().any(|s| s.name == "local_only"));
        assert_eq!(spans[2].identifiers.get("local_only"), Some(&2));
    }
}
