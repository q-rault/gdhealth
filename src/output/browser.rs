use crate::error::Result;
use crate::fs;
use crate::model::{
    DependencyKind, EntityEdge, EntityMetric, GitIntelligenceReport, GraphReport, MetricsReport,
    PolicyFinding, RefactorSuggestion, TOOL_NAME, TOOL_VERSION,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs as std_fs;
use std::path::{Path, PathBuf};

const DEFAULT_LINK_DISTANCE: f64 = 88.0;
const DEFAULT_CHARGE_FORCE: f64 = -240.0;
const DEFAULT_CLUSTER_FORCE: f64 = 0.05;
const DEFAULT_COLLISION_RADIUS: f64 = 12.0;
const DEFAULT_METRIC_RADIUS_MULTIPLIER: f64 = 1.6;

#[derive(Debug, Clone, Serialize)]
struct BrowserExport {
    metadata: BrowserMetadata,
    analysis_config: AnalysisConfig,
    graphs: GraphsExport,
    policy_summary: PolicySummaryExport,
    top_policy_findings: Vec<BrowserPolicyFinding>,
    top_functions: Vec<BrowserFunctionMetric>,
    signal_flow: crate::model::SignalFlowReport,
    godot_insights: BrowserGodotInsights,
    insights: Option<BrowserInsights>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct BrowserGodotInsights {
    autoloads: Vec<String>,
    input_actions: Vec<String>,
    input_action_uses: usize,
    group_operations: usize,
    rpc_endpoints: usize,
    rpc_calls: usize,
    exported_properties: usize,
    node_bindings: usize,
    lifecycle_callbacks: usize,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserInsights {
    git_available: bool,
    repo_bus_factor: usize,
    top_churn_files: Vec<crate::model::GitFileStats>,
    duplicates: Vec<crate::model::ClonePair>,
    refactor_suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserMetadata {
    tool_name: &'static str,
    tool_version: &'static str,
    export_format: &'static str,
    project_name: String,
    root: String,
}

#[derive(Debug, Clone, Serialize)]
struct AnalysisConfig {
    graph_types: Vec<&'static str>,
    default_graph_type: &'static str,
    default_link_distance: f64,
    default_charge_force: f64,
    default_cluster_force: f64,
    default_collision_radius: f64,
    metric_radius_multiplier: f64,
    heatmap_metric_key: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct GraphsExport {
    file_result_dependency_graph: EnrichedGraphExport,
    entity_result_dependency_graph: EnrichedGraphExport,
    entity_result_inheritance_graph: EnrichedGraphExport,
    entity_result_complete_graph: EnrichedGraphExport,
}

#[derive(Debug, Clone, Serialize)]
struct EnrichedGraphExport {
    graph: GraphPayload,
    statistics: GraphStatistics,
    overall_metric_results: OverallMetricResults,
    cluster_metrics_map: BTreeMap<String, ClusterMetrics>,
}

#[derive(Debug, Clone, Serialize)]
struct GraphPayload {
    directed: bool,
    multigraph: bool,
    graph: GraphPayloadMetadata,
    nodes: Vec<BrowserNode>,
    links: Vec<BrowserLink>,
}

#[derive(Debug, Clone, Serialize)]
struct GraphPayloadMetadata {
    name: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserNode {
    id: String,
    display_name: String,
    absolute_name: String,
    group: String,
    line_count: usize,
    function_count: usize,
    signal_count: usize,
    signal_emit_count: usize,
    signal_connection_count: usize,
    unresolved_signal_reference_count: usize,
    signal_declarations: Vec<String>,
    parser_warning_count: usize,
    has_cycle: bool,
    community_id: usize,
    metric_sloc_in_file: usize,
    metric_fan_in_dependency_graph: usize,
    metric_fan_out_dependency_graph: usize,
    metric_file_result_dependency_graph_louvain_modularity_in_file: usize,
    metric_file_hotspot_score: usize,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserLink {
    source: String,
    target: String,
    kind: String,
    line: usize,
    cross_group: bool,
}

#[derive(Debug, Clone, Serialize)]
struct GraphStatistics {
    node_count: usize,
    edge_count: usize,
    cycle_count: usize,
    parser_warning_count: usize,
    community_count: usize,
    group_count: usize,
    total_sloc: usize,
}

#[derive(Debug, Clone, Serialize)]
struct OverallMetricResults {
    metrics: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
struct ClusterMetrics {
    community_id: usize,
    label: String,
    node_count: usize,
    total_sloc: usize,
    percent_total_sloc: f64,
    avg_fan_in: f64,
    avg_fan_out: f64,
    avg_function_count: f64,
    files: Vec<String>,
}

#[derive(Debug, Clone)]
struct ClusterAccumulator {
    files: Vec<String>,
    total_sloc: usize,
    total_fan_in: usize,
    total_fan_out: usize,
    total_function_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct PolicySummaryExport {
    critical_count: usize,
    warning_count: usize,
    total_count: usize,
    cycle_count: usize,
    cross_group_dep_count: usize,
    parser_warning_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserPolicyFinding {
    severity: String,
    rule_id: String,
    message: String,
    file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BrowserFunctionMetric {
    file: String,
    function_name: String,
    hotspot_score: usize,
    cyclomatic_estimate: usize,
    line_count: usize,
    start_line: usize,
}

pub fn render_emerge_json(
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy_findings: &[PolicyFinding],
    cross_group_dep_count: usize,
) -> Result<String> {
    let export = build_export(graph, metrics, policy_findings, cross_group_dep_count);
    Ok(serde_json::to_string_pretty(&export)?)
}

/// Browser output is a health report even when requested through `graph`:
/// consumers should never lose policy, duplication, or git context merely
/// because of the output format used to request it.
pub fn render_health_emerge_json(
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy_findings: &[PolicyFinding],
    cross_group_dep_count: usize,
    git: &GitIntelligenceReport,
) -> Result<String> {
    let export = build_export_with_insights(
        graph,
        metrics,
        policy_findings,
        cross_group_dep_count,
        Some(git),
    );
    Ok(serde_json::to_string_pretty(&export)?)
}

pub fn emit_emerge_html(
    root: &Path,
    output: Option<&Path>,
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy_findings: &[PolicyFinding],
    cross_group_dep_count: usize,
) -> Result<()> {
    emit_emerge_html_with_insights(
        root,
        output,
        graph,
        metrics,
        policy_findings,
        cross_group_dep_count,
        None,
    )
}

pub fn emit_health_emerge_html(
    root: &Path,
    output: Option<&Path>,
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy_findings: &[PolicyFinding],
    cross_group_dep_count: usize,
    git: &GitIntelligenceReport,
) -> Result<()> {
    emit_emerge_html_with_insights(
        root,
        output,
        graph,
        metrics,
        policy_findings,
        cross_group_dep_count,
        Some(git),
    )
}

fn emit_emerge_html_with_insights(
    root: &Path,
    output: Option<&Path>,
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy_findings: &[PolicyFinding],
    cross_group_dep_count: usize,
    git: Option<&GitIntelligenceReport>,
) -> Result<()> {
    let export =
        build_export_with_insights(graph, metrics, policy_findings, cross_group_dep_count, git);
    let json = serde_json::to_string_pretty(&export)?;
    let (html_path, json_path) = resolve_html_output_paths(root, output);
    let html = render_html(&json, &export.metadata.project_name);

    fs::write_output(&html_path, &html)?;
    fs::write_output(&json_path, &json)?;
    Ok(())
}

fn build_export(
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy_findings: &[PolicyFinding],
    cross_group_dep_count: usize,
) -> BrowserExport {
    build_export_with_insights(graph, metrics, policy_findings, cross_group_dep_count, None)
}

fn build_export_with_insights(
    graph: &GraphReport,
    metrics: &MetricsReport,
    policy_findings: &[PolicyFinding],
    cross_group_dep_count: usize,
    git: Option<&GitIntelligenceReport>,
) -> BrowserExport {
    let godot_insights = collect_godot_insights(Path::new(&graph.root), graph);
    let project_name = derive_project_name(&graph.root);
    let parser_warning_count_by_file = build_parser_warning_counts(&graph.parser_warnings);
    let cycle_membership = build_cycle_membership_map(&graph.cycles);
    let metric_by_file = metrics
        .files
        .iter()
        .map(|metric| (metric.file.as_str(), metric))
        .collect::<BTreeMap<_, _>>();
    let community_by_file = graph
        .file_communities
        .iter()
        .map(|item| (item.id.as_str(), item.community_id))
        .collect::<BTreeMap<_, _>>();
    let file_graph_metrics = graph
        .file_graph_metrics
        .as_ref()
        .expect("file graph metrics should exist");

    let group_by_file = graph
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.group.as_deref().unwrap_or("ungrouped")))
        .collect::<BTreeMap<_, _>>();

    let nodes = graph
        .files
        .iter()
        .map(|file| {
            let metric = metric_by_file
                .get(file.path.as_str())
                .copied()
                .expect("metrics should exist for every discovered file");
            let group = file
                .group
                .clone()
                .unwrap_or_else(|| "ungrouped".to_string());
            let parser_warning_count = parser_warning_count_by_file
                .get(file.path.as_str())
                .copied()
                .unwrap_or(0);

            BrowserNode {
                id: file.id.clone(),
                display_name: display_name(&file.path),
                absolute_name: file.path.clone(),
                group,
                line_count: file.line_count,
                function_count: metric.function_count,
                signal_count: metric.signal_count,
                signal_emit_count: metric.signal_emit_count,
                signal_connection_count: metric.signal_connection_count,
                unresolved_signal_reference_count: metric.unresolved_signal_reference_count,
                signal_declarations: metrics
                    .signal_flow
                    .files
                    .iter()
                    .find(|flow| flow.file == file.path)
                    .map(|flow| flow.declarations.clone())
                    .unwrap_or_default(),
                parser_warning_count,
                has_cycle: cycle_membership.contains(file.path.as_str()),
                community_id: community_by_file
                    .get(file.path.as_str())
                    .copied()
                    .unwrap_or(0),
                metric_sloc_in_file: file.line_count,
                metric_fan_in_dependency_graph: metric.incoming_dependency_count,
                metric_fan_out_dependency_graph: metric.outgoing_dependency_count,
                metric_file_result_dependency_graph_louvain_modularity_in_file: metric
                    .louvain_modularity,
                metric_file_hotspot_score: metric.file_hotspot_score,
            }
        })
        .collect::<Vec<_>>();

    let node_id_set: BTreeSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let links = graph
        .edges
        .iter()
        .filter(|edge| {
            node_id_set.contains(edge.from.as_str()) && node_id_set.contains(edge.to.as_str())
        })
        .map(|edge| {
            let from_group = group_by_file
                .get(edge.from.as_str())
                .copied()
                .unwrap_or("ungrouped");
            let to_group = group_by_file
                .get(edge.to.as_str())
                .copied()
                .unwrap_or("ungrouped");
            BrowserLink {
                source: edge.from.clone(),
                target: edge.to.clone(),
                kind: dependency_kind_label(&edge.kind).to_string(),
                line: edge.line,
                cross_group: from_group != to_group,
            }
        })
        .collect::<Vec<_>>();
    let file_link_count = links.len();

    let total_sloc = nodes.iter().map(|node| node.line_count).sum::<usize>();
    let file_cluster_metrics_map = build_cluster_metrics(&nodes, total_sloc);
    let entity_scan = graph
        .entity_scan
        .as_ref()
        .expect("entity scan should exist");
    let entity_metric_by_id = metrics
        .entities
        .iter()
        .map(|metric| (metric.entity.as_str(), metric))
        .collect::<BTreeMap<_, _>>();
    let signal_declarations_by_file = metrics
        .signal_flow
        .files
        .iter()
        .map(|flow| (flow.file.as_str(), flow.declarations.clone()))
        .collect::<BTreeMap<_, _>>();
    let entity_dependency_export = build_entity_graph_export(
        "entity_result_dependency_graph",
        &entity_scan.entities,
        &entity_scan.dependency_edges,
        &entity_metric_by_id,
        &signal_declarations_by_file,
        &entity_scan.communities,
        entity_scan.complete_graph_metrics.total_sloc,
    );
    let entity_inheritance_export = build_entity_graph_export(
        "entity_result_inheritance_graph",
        &entity_scan.entities,
        &entity_scan.inheritance_edges,
        &entity_metric_by_id,
        &signal_declarations_by_file,
        &entity_scan.communities,
        entity_scan.complete_graph_metrics.total_sloc,
    );
    let entity_complete_export = build_entity_graph_export(
        "entity_result_complete_graph",
        &entity_scan.entities,
        &entity_scan.complete_edges,
        &entity_metric_by_id,
        &signal_declarations_by_file,
        &entity_scan.communities,
        entity_scan.complete_graph_metrics.total_sloc,
    );

    let critical_count = policy_findings
        .iter()
        .filter(|f| f.severity == "critical")
        .count();
    let warning_count = policy_findings
        .iter()
        .filter(|f| f.severity == "warning")
        .count();
    let policy_summary = PolicySummaryExport {
        critical_count,
        warning_count,
        total_count: policy_findings.len(),
        cycle_count: graph.cycles.len(),
        cross_group_dep_count,
        parser_warning_count: graph.parser_warnings.len(),
    };
    let top_policy_findings = policy_findings
        .iter()
        .take(40)
        .map(|f| BrowserPolicyFinding {
            severity: f.severity.clone(),
            rule_id: f.rule_id.clone(),
            message: f.message.clone(),
            file: f.file.clone(),
        })
        .collect();
    let top_functions = metrics
        .functions
        .iter()
        .take(12)
        .map(|f| BrowserFunctionMetric {
            file: f.file.clone(),
            function_name: f.function_name.clone(),
            hotspot_score: f.hotspot_score,
            cyclomatic_estimate: f.cyclomatic_estimate,
            line_count: f.line_count,
            start_line: f.start_line,
        })
        .collect();
    let insights = git.map(|git| BrowserInsights {
        git_available: git.available,
        repo_bus_factor: git.repo_bus_factor,
        top_churn_files: git
            .files
            .iter()
            .filter(|f| f.is_churn_hotspot)
            .take(8)
            .cloned()
            .collect(),
        duplicates: metrics.duplication.clones.iter().take(8).cloned().collect(),
        refactor_suggestions: policy_findings
            .iter()
            .filter_map(|f| {
                f.suggested_fix
                    .as_ref()
                    .map(|fix| format_refactor_suggestion(f.file.as_deref(), fix))
            })
            .take(8)
            .collect(),
    });

    BrowserExport {
        metadata: BrowserMetadata {
            tool_name: TOOL_NAME,
            tool_version: TOOL_VERSION,
            export_format: "emerge-compatible-browser",
            project_name,
            root: graph.root.clone(),
        },
        analysis_config: AnalysisConfig {
            graph_types: vec![
                "file_result_dependency_graph",
                "entity_result_dependency_graph",
                "entity_result_inheritance_graph",
                "entity_result_complete_graph",
            ],
            default_graph_type: "file_result_dependency_graph",
            default_link_distance: DEFAULT_LINK_DISTANCE,
            default_charge_force: DEFAULT_CHARGE_FORCE,
            default_cluster_force: DEFAULT_CLUSTER_FORCE,
            default_collision_radius: DEFAULT_COLLISION_RADIUS,
            metric_radius_multiplier: DEFAULT_METRIC_RADIUS_MULTIPLIER,
            heatmap_metric_key: "metric_file_hotspot_score",
        },
        graphs: GraphsExport {
            file_result_dependency_graph: EnrichedGraphExport {
                graph: GraphPayload {
                    directed: true,
                    multigraph: false,
                    graph: GraphPayloadMetadata {
                        name: "file_result_dependency_graph",
                    },
                    nodes,
                    links,
                },
                statistics: GraphStatistics {
                    node_count: graph.files.len(),
                    edge_count: file_link_count,
                    cycle_count: graph.cycles.len(),
                    parser_warning_count: graph.parser_warnings.len(),
                    community_count: file_graph_metrics.community_count,
                    group_count: file_graph_metrics.group_count,
                    total_sloc,
                },
                overall_metric_results: OverallMetricResults {
                    metrics: BTreeMap::from([
                        (
                            "metric_file_result_dependency_graph_louvain_modularity".to_string(),
                            format!("{:.6}", file_graph_metrics.louvain_modularity),
                        ),
                        (
                            "metric_file_result_dependency_graph_community_count".to_string(),
                            file_graph_metrics.community_count.to_string(),
                        ),
                        (
                            "metric_file_result_dependency_graph_total_sloc".to_string(),
                            total_sloc.to_string(),
                        ),
                        (
                            "metric_file_result_dependency_graph_total_parser_warnings".to_string(),
                            graph.parser_warnings.len().to_string(),
                        ),
                    ]),
                },
                cluster_metrics_map: file_cluster_metrics_map,
            },
            entity_result_dependency_graph: entity_dependency_export,
            entity_result_inheritance_graph: entity_inheritance_export,
            entity_result_complete_graph: entity_complete_export,
        },
        policy_summary,
        top_policy_findings,
        top_functions,
        signal_flow: metrics.signal_flow.clone(),
        godot_insights,
        insights,
    }
}

/// A deliberately conservative inventory of engine-level constructs. These
/// remain metadata until an owner/target can be proven; inventing file edges
/// for a dynamic NodePath, group, or InputMap string would make the graph lie.
fn collect_godot_insights(root: &Path, graph: &GraphReport) -> BrowserGodotInsights {
    let mut result = BrowserGodotInsights::default();
    let project = std_fs::read_to_string(root.join("project.godot")).unwrap_or_default();
    let mut input_section = false;
    let mut autoload_section = false;
    for raw in project.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            input_section = line == "[input]";
            autoload_section = line == "[autoload]";
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            let name = name.trim();
            let value = value.trim();
            if autoload_section
                && value.contains("res://")
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                result.autoloads.push(name.to_string());
            }
            if input_section
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                result.input_actions.push(name.to_string());
            }
        }
    }
    result.autoloads.sort();
    result.autoloads.dedup();
    result.input_actions.sort();
    result.input_actions.dedup();

    for file in &graph.files {
        let content = std_fs::read_to_string(root.join(&file.path)).unwrap_or_default();
        for line in content.lines() {
            let code = line.split('#').next().unwrap_or("");
            result.input_action_uses += code.matches("Input.is_action_").count()
                + code.matches("Input.get_axis(").count()
                + code.matches("Input.get_vector(").count();
            result.group_operations += code.matches("add_to_group(").count()
                + code.matches("remove_from_group(").count()
                + code.matches("get_nodes_in_group(").count()
                + code.matches("call_group(").count()
                + code.matches("call_group_flags(").count();
            result.rpc_calls += code.matches(".rpc(").count() + code.matches(".rpc_id(").count();
            result.exported_properties += code.matches("@export").count();
            result.node_bindings += code.matches("get_node(").count()
                + code.matches("get_node_or_null(").count()
                + code.matches("@onready").count()
                + code.matches('$').count();
            result.rpc_endpoints += code.matches("@rpc").count();
            for callback in [
                "_enter_tree(",
                "_ready(",
                "_exit_tree(",
                "_process(",
                "_physics_process(",
                "_input(",
                "_unhandled_input(",
            ] {
                result.lifecycle_callbacks += code.matches(callback).count();
            }
        }
    }
    result
}

fn format_refactor_suggestion(file: Option<&str>, suggestion: &RefactorSuggestion) -> String {
    let target = file.unwrap_or("project");
    match suggestion {
        RefactorSuggestion::BreakCycle { cut_edges } => {
            let edges = cut_edges
                .iter()
                .map(|(from, to)| format!("{from} → {to}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Break cycle in {target}: remove {edges}")
        }
        RefactorSuggestion::SplitFile {
            proposed_groups, ..
        } => {
            format!(
                "Split {target} into {} cohesive groups",
                proposed_groups.len()
            )
        }
    }
}

fn build_entity_graph_export(
    graph_name: &'static str,
    entities: &[crate::model::EntityNode],
    edges: &[EntityEdge],
    entity_metric_by_id: &BTreeMap<&str, &EntityMetric>,
    signal_declarations_by_file: &BTreeMap<&str, Vec<String>>,
    communities: &[crate::model::GraphCommunity],
    total_sloc: usize,
) -> EnrichedGraphExport {
    let community_by_id = communities
        .iter()
        .map(|item| (item.id.as_str(), item.community_id))
        .collect::<BTreeMap<_, _>>();
    let nodes = entities
        .iter()
        .map(|entity| {
            let metric = entity_metric_by_id
                .get(entity.id.as_str())
                .copied()
                .expect("entity metrics should exist for every entity");
            BrowserNode {
                id: entity.id.clone(),
                display_name: entity.display_name.clone(),
                absolute_name: entity.file.clone(),
                group: Path::new(&entity.file)
                    .parent()
                    .map(|parent| parent.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|| "root".to_string()),
                line_count: entity.line_count,
                function_count: entity.method_count,
                signal_count: metric.signal_count,
                signal_emit_count: metric.signal_emit_count,
                signal_connection_count: metric.signal_connection_count,
                unresolved_signal_reference_count: metric.unresolved_signal_reference_count,
                signal_declarations: signal_declarations_by_file
                    .get(entity.file.as_str())
                    .cloned()
                    .unwrap_or_default(),
                parser_warning_count: 0,
                has_cycle: false,
                community_id: community_by_id
                    .get(entity.id.as_str())
                    .copied()
                    .unwrap_or(0),
                metric_sloc_in_file: entity.line_count,
                metric_fan_in_dependency_graph: metric.incoming_dependency_count,
                metric_fan_out_dependency_graph: metric.outgoing_dependency_count,
                metric_file_result_dependency_graph_louvain_modularity_in_file: metric
                    .louvain_modularity,
                metric_file_hotspot_score: metric.ws_complexity,
            }
        })
        .collect::<Vec<_>>();
    let entity_id_set: BTreeSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let links = edges
        .iter()
        .filter(|edge| {
            entity_id_set.contains(edge.from.as_str()) && entity_id_set.contains(edge.to.as_str())
        })
        .map(|edge| BrowserLink {
            source: edge.from.clone(),
            target: edge.to.clone(),
            kind: dependency_kind_label(&edge.kind).to_string(),
            line: edge.line,
            cross_group: false,
        })
        .collect::<Vec<_>>();
    let entity_link_count = links.len();
    let cluster_metrics_map = build_cluster_metrics(&nodes, total_sloc);
    let community_count = communities
        .iter()
        .map(|item| item.community_id)
        .collect::<BTreeSet<_>>()
        .len();

    EnrichedGraphExport {
        graph: GraphPayload {
            directed: true,
            multigraph: false,
            graph: GraphPayloadMetadata { name: graph_name },
            nodes,
            links,
        },
        statistics: GraphStatistics {
            node_count: entities.len(),
            edge_count: entity_link_count,
            cycle_count: 0,
            parser_warning_count: 0,
            community_count,
            group_count: 0,
            total_sloc,
        },
        overall_metric_results: OverallMetricResults {
            metrics: BTreeMap::from([
                (
                    format!("metric_{graph_name}_community_count"),
                    community_count.to_string(),
                ),
                (
                    format!("metric_{graph_name}_total_sloc"),
                    total_sloc.to_string(),
                ),
            ]),
        },
        cluster_metrics_map,
    }
}

fn resolve_html_output_paths(root: &Path, output: Option<&Path>) -> (PathBuf, PathBuf) {
    let default_dir = root.join("reports").join("code-health").join("browser");
    match output {
        None => (
            default_dir.join("emerge.html"),
            default_dir.join("graph-emerge.json"),
        ),
        Some(path) if path.extension().is_some_and(|ext| ext == "html") => {
            (path.to_path_buf(), path.with_file_name("graph-emerge.json"))
        }
        Some(path) => (path.join("emerge.html"), path.join("graph-emerge.json")),
    }
}

fn derive_project_name(root: &str) -> String {
    let path = Path::new(root);
    if let Ok(canonical) = path.canonicalize() {
        if let Some(name) = canonical.file_name() {
            let value = name.to_string_lossy().trim().to_string();
            if !value.is_empty() {
                return value;
            }
        }
    }

    if let Some(name) = path.file_name() {
        let value = name.to_string_lossy().trim().to_string();
        if !value.is_empty() {
            return value;
        }
    }

    "gdhealth-project".to_string()
}

fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn dependency_kind_label(kind: &DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Extends => "extends",
        DependencyKind::Preload => "preload",
        DependencyKind::Load => "load",
        DependencyKind::ConstPreload => "const_preload",
        DependencyKind::UnknownScriptReference => "unknown_script_reference",
    }
}

fn build_parser_warning_counts(warnings: &[String]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for warning in warnings {
        if let Some((file, _)) = warning.split_once(':') {
            *counts.entry(file.to_string()).or_insert(0) += 1;
        }
    }
    counts
}

fn build_cycle_membership_map(cycles: &[Vec<String>]) -> BTreeSet<&str> {
    cycles
        .iter()
        .flat_map(|cycle| cycle.iter().map(String::as_str))
        .collect()
}

fn build_cluster_metrics(
    nodes: &[BrowserNode],
    total_sloc: usize,
) -> BTreeMap<String, ClusterMetrics> {
    let mut accumulators = BTreeMap::<usize, ClusterAccumulator>::new();

    for node in nodes {
        let entry = accumulators
            .entry(node.community_id)
            .or_insert_with(|| ClusterAccumulator {
                files: Vec::new(),
                total_sloc: 0,
                total_fan_in: 0,
                total_fan_out: 0,
                total_function_count: 0,
            });
        entry.files.push(node.absolute_name.clone());
        entry.total_sloc += node.line_count;
        entry.total_fan_in += node.metric_fan_in_dependency_graph;
        entry.total_fan_out += node.metric_fan_out_dependency_graph;
        entry.total_function_count += node.function_count;
    }

    let mut result: BTreeMap<String, ClusterMetrics> = accumulators
        .into_iter()
        .map(|(community_id, mut accumulator)| {
            accumulator.files.sort();
            let node_count = accumulator.files.len();
            let percent_total_sloc = if total_sloc == 0 {
                0.0
            } else {
                (accumulator.total_sloc as f64 / total_sloc as f64) * 100.0
            };
            (
                community_id.to_string(),
                ClusterMetrics {
                    community_id,
                    label: derive_cluster_label(&accumulator.files),
                    node_count,
                    total_sloc: accumulator.total_sloc,
                    percent_total_sloc,
                    avg_fan_in: average(accumulator.total_fan_in, node_count),
                    avg_fan_out: average(accumulator.total_fan_out, node_count),
                    avg_function_count: average(accumulator.total_function_count, node_count),
                    files: accumulator.files,
                },
            )
        })
        .collect();
    deduplicate_cluster_labels(&mut result);
    result
}

fn deduplicate_cluster_labels(map: &mut BTreeMap<String, ClusterMetrics>) {
    let mut label_counts: BTreeMap<String, usize> = BTreeMap::new();
    for metrics in map.values() {
        *label_counts.entry(metrics.label.clone()).or_insert(0) += 1;
    }
    for metrics in map.values_mut() {
        if label_counts.get(&metrics.label).copied().unwrap_or(0) > 1 {
            metrics.label = format!("{} #{}", metrics.label, metrics.community_id);
        }
    }
}

fn derive_cluster_label(files: &[String]) -> String {
    if files.is_empty() {
        return "ungrouped".to_string();
    }

    if files.len() == 1 {
        let path = Path::new(&files[0]);
        return path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| files[0].clone());
    }

    let directories = files
        .iter()
        .map(|path| {
            Path::new(path)
                .parent()
                .map(|parent| parent.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();

    let mut common_segments = directories
        .first()
        .map(|value| {
            value
                .split('/')
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for directory in directories.iter().skip(1) {
        let segments = directory
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        let shared_length = common_segments
            .iter()
            .zip(segments.iter())
            .take_while(|(left, right)| left == right)
            .count();
        common_segments.truncate(shared_length);
        if common_segments.is_empty() {
            break;
        }
    }

    if !common_segments.is_empty() {
        return common_segments.join("/");
    }

    let mut counts = BTreeMap::<String, usize>::new();
    for directory in directories {
        let label = if directory.is_empty() {
            "root".to_string()
        } else {
            directory
                .split('/')
                .next()
                .map(str::to_string)
                .unwrap_or_else(|| "root".to_string())
        };
        *counts.entry(label).or_insert(0) += 1;
    }

    counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(label, _)| label)
        .unwrap_or_else(|| "ungrouped".to_string())
}

fn average(total: usize, count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn render_html(data_json: &str, project_name: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{project_name} — gdhealth</title>
  <style>
    :root {{
      --bg: #0F0F11;
      --panel: #16161A;
      --panel-2: #1E1E23;
      --panel-3: #26262D;
      --border: rgba(255,255,255,0.07);
      --border-2: rgba(255,255,255,0.13);
      --text: #EDEDF0;
      --text-2: #9090A0;
      --text-3: #56566A;
      --accent: #5E6AD2;
      --accent-bg: rgba(94,106,210,0.13);
      --red: #E05252;
      --red-bg: rgba(224,82,82,0.11);
      --yellow: #D9813A;
      --yellow-bg: rgba(217,129,58,0.11);
      --green: #3FC880;
      --green-bg: rgba(63,200,128,0.11);
      --orange: #EA783C;
      --orange-bg: rgba(234,120,60,0.11);
      --grid: rgba(255,255,255,0.032);
      --edge-default: rgba(255,255,255,0.15);
      --edge-cross: rgba(217,129,58,0.55);
    }}
    *, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
    html, body {{ height: 100%; overflow: hidden; }}
    body {{
      font-family: -apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", "Helvetica Neue", Arial, sans-serif;
      font-size: 13px;
      line-height: 1.4;
      background: var(--bg);
      color: var(--text);
    }}
    .app {{
      display: grid;
      grid-template-columns: 256px 1fr 284px;
      height: 100vh;
      gap: 1px;
      background: var(--border);
    }}
    .panel {{
      background: var(--panel);
      display: flex;
      flex-direction: column;
      min-height: 0;
      overflow: hidden;
    }}
    .panel-header {{
      padding: 16px 16px 12px;
      border-bottom: 1px solid var(--border);
      flex: none;
    }}
    .tool-label {{
      font-size: 10px;
      font-weight: 600;
      color: var(--accent);
      text-transform: uppercase;
      letter-spacing: 0.1em;
      display: block;
      margin-bottom: 3px;
    }}
    .project-name {{
      font-size: 15px;
      font-weight: 600;
      color: var(--text);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }}
    .panel-body {{
      flex: 1;
      overflow-y: auto;
      padding: 12px;
      display: flex;
      flex-direction: column;
      gap: 14px;
      min-height: 0;
    }}
    .section {{ display: flex; flex-direction: column; gap: 7px; }}
    .section-label {{
      font-size: 10.5px;
      font-weight: 500;
      color: var(--text-3);
      text-transform: uppercase;
      letter-spacing: 0.07em;
      padding-bottom: 7px;
      border-bottom: 1px solid var(--border);
      display: flex;
      align-items: center;
      justify-content: space-between;
    }}
    .has-tooltip {{ cursor: help; border-bottom: 1px dotted var(--text-3); }}
    #gdhealth-tooltip {{ position: fixed; z-index: 1000; width: min(320px, calc(100vw - 16px)); padding: 8px 10px; border: 1px solid var(--border-2); border-radius: 7px; background: #24242B; color: var(--text); font-size: 11px; font-weight: 400; line-height: 1.45; letter-spacing: 0; text-transform: none; box-shadow: 0 8px 24px rgba(0,0,0,.35); pointer-events: none; overflow: hidden; visibility: hidden; }}
    #gdhealth-tooltip-body {{ overflow-y: auto; max-height: inherit; }}
    .section-aside {{
      font-size: 10px;
      font-weight: 400;
      color: var(--text-3);
      text-transform: none;
      letter-spacing: 0;
    }}
    select.select {{
      width: 100%;
      background: var(--panel-2);
      border: 1px solid var(--border);
      border-radius: 5px;
      padding: 7px 10px;
      font: inherit;
      font-size: 12px;
      color: var(--text);
      cursor: pointer;
      outline: none;
      appearance: auto;
    }}
    select.select:focus {{ border-color: var(--accent); }}
    input.search {{
      width: 100%;
      background: var(--panel-2);
      border: 1px solid var(--border);
      border-radius: 5px;
      padding: 7px 10px;
      font: inherit;
      font-size: 12px;
      color: var(--text);
      outline: none;
    }}
    input.search::placeholder {{ color: var(--text-3); }}
    input.search:focus {{ border-color: var(--accent); }}
    .stats-grid {{
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 5px;
    }}
    .stat-card {{
      background: var(--panel-2);
      border: 1px solid var(--border);
      border-radius: 5px;
      padding: 9px 10px;
      cursor: default;
    }}
    .stat-label {{
      font-size: 10px;
      color: var(--text-3);
      font-weight: 500;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    .stat-value {{
      font-size: 20px;
      font-weight: 600;
      line-height: 1.1;
      margin-top: 2px;
      font-variant-numeric: tabular-nums;
    }}
    .badge-row {{
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
    }}
    .badge {{
      display: inline-flex;
      align-items: center;
      gap: 4px;
      padding: 2px 7px;
      border-radius: 4px;
      font-size: 11px;
      font-weight: 500;
      white-space: nowrap;
    }}
    .badge-dot {{
      width: 5px;
      height: 5px;
      border-radius: 50%;
      flex: none;
    }}
    .badge-red {{ background: var(--red-bg); color: var(--red); }}
    .badge-red .badge-dot {{ background: var(--red); }}
    .badge-yellow {{ background: var(--yellow-bg); color: var(--yellow); }}
    .badge-yellow .badge-dot {{ background: var(--yellow); }}
    .badge-orange {{ background: var(--orange-bg); color: var(--orange); }}
    .badge-orange .badge-dot {{ background: var(--orange); }}
    .badge-blue {{ background: var(--accent-bg); color: var(--accent); }}
    .badge-blue .badge-dot {{ background: var(--accent); }}
    .seg-control {{
      display: grid;
      grid-template-columns: 1fr 1fr;
      background: var(--panel-2);
      border: 1px solid var(--border);
      border-radius: 5px;
      padding: 2px;
      gap: 2px;
    }}
    .seg {{
      padding: 5px;
      background: transparent;
      border: none;
      border-radius: 3px;
      font: inherit;
      font-size: 12px;
      color: var(--text-2);
      cursor: pointer;
      transition: background 0.12s, color 0.12s;
      text-align: center;
    }}
    .seg:hover {{ background: var(--panel-3); color: var(--text); }}
    .seg.active {{ background: var(--panel-3); color: var(--text); }}
    .cluster-list, .scroll-list {{ display: flex; flex-direction: column; gap: 1px; }}
    .cluster-item {{
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 6px 8px;
      border-radius: 4px;
      cursor: pointer;
      font-size: 12px;
      color: var(--text-2);
      transition: background 0.1s, color 0.1s;
      border: 1px solid transparent;
    }}
    .cluster-item:hover {{ background: var(--panel-2); color: var(--text); }}
    .cluster-item.active {{ background: var(--accent-bg); color: var(--text); border-color: rgba(94,106,210,0.2); }}
    .cluster-dot {{
      width: 7px;
      height: 7px;
      border-radius: 50%;
      flex: none;
    }}
    .cluster-name {{ flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 500; }}
    .cluster-meta {{ font-size: 10.5px; color: var(--text-3); flex: none; }}
    .canvas-panel {{
      background: var(--bg);
      position: relative;
    }}
    .canvas-panel::before {{
      content: "";
      position: absolute;
      inset: 0;
      background-image:
        linear-gradient(var(--grid) 1px, transparent 1px),
        linear-gradient(90deg, var(--grid) 1px, transparent 1px);
      background-size: 24px 24px;
      pointer-events: none;
      z-index: 0;
    }}
    svg {{
      width: 100%;
      height: 100%;
      display: block;
      position: relative;
      z-index: 1;
    }}
    .canvas-overlay {{
      position: absolute;
      z-index: 10;
      background: rgba(22,22,26,0.9);
      border: 1px solid var(--border);
      border-radius: 5px;
      padding: 6px 10px;
      font-size: 11px;
      color: var(--text-3);
      backdrop-filter: blur(4px);
    }}
    .canvas-overlay.bottom-left {{ bottom: 12px; left: 12px; }}
    .canvas-overlay.bottom-right {{ bottom: 12px; right: 12px; }}
    .canvas-overlay.hidden {{ display: none; }}
    .heatbar {{
      width: 110px;
      height: 5px;
      border-radius: 3px;
      background: linear-gradient(to right, #3FC880, #D9813A, #E05252);
      margin-bottom: 4px;
    }}
    .heatbar-labels {{
      display: flex;
      justify-content: space-between;
      font-size: 10px;
      color: var(--text-3);
    }}
    .empty-state {{
      font-size: 12px;
      color: var(--text-3);
      padding: 6px 0;
    }}
    .metric-grid {{
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 5px;
    }}
    .metric-card {{
      background: var(--panel-2);
      border: 1px solid var(--border);
      border-radius: 5px;
      padding: 8px 10px;
    }}
    .metric-label {{
      font-size: 10px;
      color: var(--text-3);
      text-transform: uppercase;
      letter-spacing: 0.05em;
      font-weight: 500;
    }}
    .metric-value {{
      font-size: 17px;
      font-weight: 600;
      margin-top: 2px;
      font-variant-numeric: tabular-nums;
    }}
    .node-name {{
      font-size: 13px;
      font-weight: 600;
      color: var(--text);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      margin-bottom: 2px;
    }}
    .node-path {{
      font-size: 11px;
      color: var(--text-3);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      margin-bottom: 8px;
    }}
    .node-tags {{
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
      margin-top: 6px;
    }}
    .tag {{
      font-size: 10.5px;
      padding: 1px 6px;
      border-radius: 3px;
      font-weight: 500;
    }}
    .tag-red {{ background: var(--red-bg); color: var(--red); }}
    .tag-blue {{ background: var(--accent-bg); color: var(--accent); }}
    .tag-yellow {{ background: var(--yellow-bg); color: var(--yellow); }}
    .finding-list {{ display: flex; flex-direction: column; gap: 1px; }}
    .finding-item {{
      display: flex;
      gap: 8px;
      padding: 5px 6px;
      border-radius: 4px;
      cursor: default;
      transition: background 0.1s;
    }}
    .finding-item:hover {{ background: var(--panel-2); }}
    .finding-dot {{
      width: 6px;
      height: 6px;
      border-radius: 50%;
      margin-top: 4px;
      flex: none;
    }}
    .finding-dot.critical {{ background: var(--red); }}
    .finding-dot.warning {{ background: var(--yellow); }}
    .finding-body {{ flex: 1; min-width: 0; }}
    .finding-msg {{
      font-size: 11.5px;
      color: var(--text-2);
      line-height: 1.5;
    }}
    .finding-file {{
      font-size: 10.5px;
      color: var(--text-3);
      margin-top: 1px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }}
    .fn-list {{ display: flex; flex-direction: column; gap: 1px; }}
    .fn-item {{
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 5px 6px;
      border-radius: 4px;
      cursor: default;
      transition: background 0.1s;
    }}
    .fn-item:hover {{ background: var(--panel-2); }}
    .fn-body {{ flex: 1; min-width: 0; }}
    .fn-name {{
      font-size: 12px;
      color: var(--text);
      font-weight: 500;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }}
    .fn-file {{
      font-size: 10.5px;
      color: var(--text-3);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }}
    .fn-score {{
      font-size: 11px;
      font-weight: 600;
      padding: 2px 6px;
      border-radius: 4px;
      flex: none;
      font-variant-numeric: tabular-nums;
    }}
    .fn-score-high {{ background: var(--red-bg); color: var(--red); }}
    .fn-score-mid {{ background: var(--yellow-bg); color: var(--yellow); }}
    .fn-score-low {{ background: var(--green-bg); color: var(--green); }}
    .divider {{
      height: 1px;
      background: var(--border);
      flex: none;
    }}
    .relation-filters {{ display:flex; flex-wrap:wrap; gap:5px; margin:8px 0 10px; }}
    .relation-filter {{ font-size:11px; padding:4px 7px; border:1px solid var(--border-2); border-radius:999px; background:transparent; color:var(--text-2); cursor:pointer; }}
    .relation-filter.active {{ background:var(--blue-bg); border-color:var(--blue); color:var(--blue); }}
    ::-webkit-scrollbar {{ width: 3px; }}
    ::-webkit-scrollbar-thumb {{ background: var(--border-2); border-radius: 2px; }}
    ::-webkit-scrollbar-track {{ background: transparent; }}
    @media (max-width: 900px) {{
      .app {{ grid-template-columns: 1fr; height: auto; overflow: auto; }}
      html, body {{ height: auto; overflow: auto; }}
      .canvas-panel {{ height: 60vh; }}
      .panel-body {{ max-height: none; overflow: visible; }}
    }}
  </style>
</head>
<body>
  <div class="app">
    <aside class="panel">
      <div class="panel-header">
        <span class="tool-label">gdhealth</span>
        <div class="project-name" title="{project_name}">{project_name}</div>
      </div>
      <div class="panel-body">
        <div class="section">
          <div class="section-label"><span class="has-tooltip" data-tooltip="One file-centric architecture graph. Toggle relationship categories without changing graph views.">Architecture relations</span></div>
          <div id="relation-filters" class="relation-filters"></div>
          <input id="search" class="search" type="search" placeholder="Search files or groups…">
        </div>
        <div class="section">
          <div class="section-label">Overview</div>
          <div id="stats" class="stats-grid"></div>
          <div id="health-badges" class="badge-row"></div>
        </div>
        <div class="section">
          <div class="section-label">Color</div>
          <div class="seg-control">
            <button class="seg active" id="mode-community">Community</button>
            <button class="seg" id="mode-hotspot">Hotspot</button>
          </div>
        </div>
        <div class="section">
          <div class="section-label">Clusters</div>
          <div id="clusters" class="cluster-list" style="height:260px;overflow-y:auto;"></div>
        </div>
      </div>
    </aside>

    <main class="panel canvas-panel">
      <svg id="graph" viewBox="0 0 960 720" preserveAspectRatio="xMidYMid meet"></svg>
      <div class="canvas-overlay bottom-right" style="display:flex;gap:4px;align-items:center;bottom:auto;top:12px;"><button class="seg" id="zoom-out" title="Zoom out">−</button><button class="seg" id="zoom-reset" title="Reset zoom">100%</button><button class="seg" id="zoom-in" title="Zoom in">+</button></div>
      <div class="canvas-overlay bottom-left">Drag · Click to select · Esc to clear</div>
      <div class="canvas-overlay bottom-right hidden" id="hotspot-legend">
        <div class="heatbar"></div>
        <div class="heatbar-labels"><span>Low</span><span>High</span></div>
      </div>
    </main>

    <aside class="panel">
      <div class="panel-body">
        <div class="section" id="selection-section">
          <div class="section-label">Selection</div>
          <div id="selection" class="empty-state">Click a node or cluster to inspect.</div>
        </div>
        <div class="divider"></div>
        <div class="section">
          <div class="section-label">Signal flow</div>
          <div id="signal-flow" class="scroll-list"></div>
        </div>
        <div class="divider"></div>
        <div class="section">
          <div class="section-label"><span class="has-tooltip" data-tooltip="Godot engine contracts found statically. They are shown as inventory until gdhealth can prove a concrete file-to-file runtime target.">Godot architecture</span></div>
          <div id="godot-insights" class="fn-list"></div>
        </div>
        <div class="divider"></div>
        <div class="section" style="flex:1;min-height:0;min-height:140px;">
          <div class="section-label">
            <span class="has-tooltip" data-tooltip="Rules that exceed configured architecture or complexity thresholds. Critical findings need immediate attention; warnings should be reviewed.">Policy findings</span>
            <span class="section-aside" id="policy-count-aside"></span>
          </div>
          <div id="policy-findings" class="finding-list" style="overflow-y:auto;flex:1;"></div>
        </div>
        <div class="divider"></div>
        <div class="section">
          <div class="section-label"><span class="has-tooltip" data-tooltip="Functions with the highest structural hotspot score: a combination of size, complexity, and dependency coupling.">Hot functions</span></div>
          <div id="hot-functions" class="fn-list"></div>
        </div>
        <div class="divider"></div>
        <div class="section">
          <div class="section-label"><span class="has-tooltip" data-tooltip="History-derived signals: recent change frequency, ownership concentration, defect-shaped commits, and co-change patterns.">Git signals</span></div>
          <div id="git-insights" class="fn-list"></div>
        </div>
        <div class="section">
          <div class="section-label"><span class="has-tooltip" data-tooltip="Similar normalized code blocks. Variable names are ignored so copy-paste variants remain visible.">Duplicate code</span></div>
          <div id="duplicate-insights" class="fn-list"></div>
        </div>
        <div class="section">
          <div class="section-label"><span class="has-tooltip" data-tooltip="Concrete, conservative suggestions generated from cycles and cohesive clusters inside large files.">Refactoring suggestions</span></div>
          <div id="refactor-insights" class="fn-list"></div>
        </div>
      </div>
    </aside>
  </div>

  <script id="graph-data" type="application/json">{data_json}</script>
  <div id="gdhealth-tooltip" role="tooltip"><div id="gdhealth-tooltip-body"></div></div>
  <script>
    const payload = JSON.parse(document.getElementById('graph-data').textContent);
    const relationLabels = {{ extends: 'Inheritance', preload: 'Preload', const_preload: 'Const preload', load: 'Load', unknown_script_reference: 'Class reference' }};

    const W = 960, H = 720;
    let graphPayload = null;
    let nodes = [], nodeMap = new Map(), links = [], clusters = [], stats = null;
    let activeCluster = null, activeNodeId = null, draggingNode = null;
    let colorMode = 'community';
    let maxHotspot = 1;
    let searchTerm = '';
    let zoom = 1;
    let zoomCenter = {{ x: W / 2, y: H / 2 }};

    const selEl = document.getElementById('selection');
    const clustersEl = document.getElementById('clusters');
    const statsEl = document.getElementById('stats');
    const badgesEl = document.getElementById('health-badges');
    const relationFiltersEl = document.getElementById('relation-filters');
    const searchEl = document.getElementById('search');
    const policyEl = document.getElementById('policy-findings');
    const hotFnEl = document.getElementById('hot-functions');
    const gitInsightsEl = document.getElementById('git-insights');
    const duplicateInsightsEl = document.getElementById('duplicate-insights');
    const refactorInsightsEl = document.getElementById('refactor-insights');
    const signalFlowEl = document.getElementById('signal-flow');
    const godotInsightsEl = document.getElementById('godot-insights');
    const policyAside = document.getElementById('policy-count-aside');
    const hotspotLegend = document.getElementById('hotspot-legend');
    const svg = document.getElementById('graph');
    const linkLayer = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    const nodeLayer = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    svg.append(linkLayer, nodeLayer);

    function applyZoom() {{
      const width = W / zoom, height = H / zoom;
      const x = Math.max(0, Math.min(W - width, zoomCenter.x - width / 2));
      const y = Math.max(0, Math.min(H - height, zoomCenter.y - height / 2));
      svg.setAttribute('viewBox', `${{x}} ${{y}} ${{width}} ${{height}}`);
      document.getElementById('zoom-reset').textContent = `${{Math.round(zoom * 100)}}%`;
    }}

    // ── color helpers ──────────────────────────────────────────────
    function communityColor(id) {{
      const h = (id * 57 + 180) % 360;
      return `hsl(${{h}} 50% 62%)`;
    }}

    function lerpColor(r1,g1,b1, r2,g2,b2, t) {{
      return [r1+(r2-r1)*t|0, g1+(g2-g1)*t|0, b1+(b2-b1)*t|0];
    }}

    function hotspotColor(score) {{
      const t = maxHotspot > 0 ? Math.min(score / maxHotspot, 1) : 0;
      let r, g, b;
      if (t < 0.5) {{
        [r,g,b] = lerpColor(63,200,128, 217,129,58, t*2);
      }} else {{
        [r,g,b] = lerpColor(217,129,58, 224,82,82, (t-0.5)*2);
      }}
      return `rgb(${{r}},${{g}},${{b}})`;
    }}

    function nodeColor(node) {{
      return colorMode === 'hotspot'
        ? hotspotColor(node.metric_file_hotspot_score)
        : communityColor(node.community_id);
    }}

    function fmt(v) {{
      return typeof v === 'number' && !Number.isInteger(v) ? v.toFixed(2) : String(v);
    }}

    const tooltip = document.getElementById('gdhealth-tooltip');
    const tooltipBody = document.getElementById('gdhealth-tooltip-body');
    let tooltipTrigger = null;
    function placeTooltip() {{
      if (!tooltipTrigger) return;
      const rect = tooltipTrigger.getBoundingClientRect();
      tooltip.style.maxHeight = `${{Math.max(1, window.innerHeight - 16)}}px`;
      tooltipBody.style.maxHeight = `${{Math.max(1, window.innerHeight - 32)}}px`;
      tooltip.style.visibility = 'hidden';
      tooltip.style.display = 'block';
      const tip = tooltip.getBoundingClientRect();
      const gutter = 8;
      let top = rect.top - tip.height - 8;
      if (top < gutter) top = rect.bottom + 8;
      top = Math.max(gutter, Math.min(top, window.innerHeight - tip.height - gutter));
      const left = Math.max(gutter, Math.min(rect.left + rect.width / 2 - tip.width / 2, window.innerWidth - tip.width - gutter));
      tooltip.style.left = `${{left}}px`;
      tooltip.style.top = `${{top}}px`;
      tooltip.style.visibility = 'visible';
    }}
    function showTooltip(trigger) {{
      const message = trigger.dataset.tooltip;
      if (!message) return;
      tooltipTrigger = trigger;
      tooltipBody.textContent = message;
      trigger.setAttribute('aria-describedby', 'gdhealth-tooltip');
      placeTooltip();
    }}
    function hideTooltip() {{
      if (tooltipTrigger) tooltipTrigger.removeAttribute('aria-describedby');
      tooltipTrigger = null;
      tooltip.style.display = 'none';
      tooltip.style.visibility = 'hidden';
    }}
    document.addEventListener('mouseover', event => {{ const trigger = event.target.closest('[data-tooltip]'); if (trigger) showTooltip(trigger); }});
    document.addEventListener('mouseout', event => {{ const trigger = event.target.closest('[data-tooltip]'); if (trigger && !trigger.contains(event.relatedTarget)) hideTooltip(); }});
    document.addEventListener('focusin', event => {{ const trigger = event.target.closest('[data-tooltip]'); if (trigger) showTooltip(trigger); }});
    document.addEventListener('focusout', event => {{ if (tooltipTrigger && !tooltipTrigger.contains(event.relatedTarget)) hideTooltip(); }});
    document.addEventListener('pointerdown', event => {{ const trigger = event.target.closest('[data-tooltip]'); if (trigger && event.pointerType !== 'mouse') showTooltip(trigger); else if (!trigger) hideTooltip(); }});
    document.addEventListener('keydown', event => {{ if (event.key === 'Escape') hideTooltip(); }});
    window.addEventListener('resize', placeTooltip);
    window.addEventListener('orientationchange', placeTooltip);
    window.addEventListener('scroll', placeTooltip, true);

    // ── render: stats ──────────────────────────────────────────────
    let activeKinds = new Set();
    function visibleLinks() {{ return links.filter(lk => activeKinds.has(lk.kind)); }}
    function renderRelationFilters() {{
      const kinds = [...new Set(links.map(lk => lk.kind))].sort();
      if (!activeKinds.size) activeKinds = new Set(kinds);
      relationFiltersEl.innerHTML = kinds.map(kind => `<button class="relation-filter${{activeKinds.has(kind) ? ' active' : ''}}" data-kind="${{kind}}">${{relationLabels[kind] || kind}}</button>`).join('');
      relationFiltersEl.querySelectorAll('[data-kind]').forEach(button => button.addEventListener('click', () => {{
        const kind = button.dataset.kind;
        if (activeKinds.has(kind)) activeKinds.delete(kind); else activeKinds.add(kind);
        renderRelationFilters(); renderStats(); draw();
      }}));
    }}
    function renderStats() {{
      const ps = payload.policy_summary;
      statsEl.innerHTML = [
        ['Files', stats.node_count, 'Source files analyzed in this graph.'],
        ['Edges', new Set(visibleLinks().map(l => `${{l.source}}→${{l.target}}`)).size, 'Unique resolved inter-file file pairs visible under the selected relationship filters. A declaration alone is not an edge.'],
        ['Relations', visibleLinks().length, 'Resolved relationship occurrences visible under the selected filters.'],
        ['Clusters', stats.community_count, 'Communities detected by Louvain clustering: files that are more connected to each other than to the rest of the project.'],
        ['SLOC', stats.total_sloc.toLocaleString(), 'Source lines of code: physical lines in analyzed scripts, excluding non-code project assets.'],
        ['Signal declarations', nodes.reduce((total, node) => total + node.signal_count, 0), 'GDScript signal declarations. Contextual only; they do not alter hotspot scores in this release.'],
        ['Signal activity', nodes.reduce((total, node) => total + node.signal_emit_count + node.signal_connection_count, 0), 'Recognized signal emits and connections. They become graph relations only when their external owner and callback target can be resolved.'],
      ].map(([l,v,tip]) => `<div class="stat-card"><div class="stat-label has-tooltip" data-tooltip="${{tip}}">${{l}}</div><div class="stat-value">${{v}}</div></div>`).join('');

      badgesEl.innerHTML = [
        ps.critical_count > 0 ? `<span class="badge badge-red"><span class="badge-dot"></span>Critical ${{ps.critical_count}}</span>` : '',
        ps.warning_count > 0 ? `<span class="badge badge-yellow"><span class="badge-dot"></span>Warning ${{ps.warning_count}}</span>` : '',
        ps.cycle_count > 0 ? `<span class="badge badge-orange"><span class="badge-dot"></span>Cycles ${{ps.cycle_count}}</span>` : '',
        ps.cross_group_dep_count > 0 ? `<span class="badge badge-blue"><span class="badge-dot"></span>Cross-group ${{ps.cross_group_dep_count}}</span>` : '',
      ].filter(Boolean).join('');
    }}

    // ── render: clusters ───────────────────────────────────────────
    function renderClusters() {{
      clustersEl.innerHTML = clusters.map(c => `
        <button class="cluster-item${{activeCluster === c.community_id ? ' active' : ''}}" data-cid="${{c.community_id}}">
          <span class="cluster-dot" style="background:${{communityColor(c.community_id)}}"></span>
          <span class="cluster-name">${{c.label || 'Cluster ' + c.community_id}}</span>
          <span class="cluster-meta">${{c.node_count}} files</span>
        </button>
      `).join('');
      clustersEl.querySelectorAll('[data-cid]').forEach(btn => {{
        btn.addEventListener('click', () => {{
          const cid = Number(btn.dataset.cid);
          activeCluster = activeCluster === cid ? null : cid;
          activeNodeId = null;
          renderClusters();
          updateSelection(null);
          draw();
        }});
      }});
    }}

    // ── render: selection ──────────────────────────────────────────
    function updateSelection(node) {{
      if (!node && activeCluster == null) {{
        selEl.className = 'empty-state';
        selEl.textContent = 'Click a node or cluster to inspect.';
        return;
      }}
      if (!node && activeCluster != null) {{
        const c = clusters.find(x => x.community_id === activeCluster);
        if (!c) return;
        selEl.className = '';
        selEl.innerHTML = `
          <div class="node-name">${{c.label || 'Cluster ' + c.community_id}}</div>
          <div class="node-path">${{c.node_count}} files · ${{c.total_sloc}} SLOC · ${{c.percent_total_sloc.toFixed(1)}}%</div>
          <div class="metric-grid">
            <div class="metric-card" title="Average fan-in"><div class="metric-label">Avg fan-in</div><div class="metric-value">${{fmt(c.avg_fan_in)}}</div></div>
            <div class="metric-card" title="Average fan-out"><div class="metric-label">Avg fan-out</div><div class="metric-value">${{fmt(c.avg_fan_out)}}</div></div>
            <div class="metric-card" title="Average functions per file"><div class="metric-label">Avg funcs</div><div class="metric-value">${{fmt(c.avg_function_count)}}</div></div>
            <div class="metric-card"><div class="metric-label">Files</div><div class="metric-value">${{c.node_count}}</div></div>
          </div>
        `;
        return;
      }}
      if (!node) return;
      const tags = [];
      if (node.has_cycle) tags.push(`<span class="tag tag-red">cycle</span>`);
      if (node.parser_warning_count > 0) tags.push(`<span class="tag tag-yellow">${{node.parser_warning_count}} warnings</span>`);
      if (node.community_id != null) tags.push(`<span class="tag tag-blue">cluster ${{node.community_id}}</span>`);
      selEl.className = '';
      selEl.innerHTML = `
        <div class="node-name" title="${{node.absolute_name}}">${{node.display_name}}</div>
        <div class="node-path">${{node.absolute_name}}</div>
        <div class="metric-grid">
          <div class="metric-card" title="Total lines in this file."><div class="metric-label">Lines</div><div class="metric-value">${{node.line_count}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="Fan-in: how many analyzed files depend on this one. High fan-in means a widely reused, potentially sensitive component.">Fan-in</div><div class="metric-value">${{node.metric_fan_in_dependency_graph}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="Fan-out: how many analyzed files this one depends on. High fan-out can indicate too many responsibilities or tight coupling.">Fan-out</div><div class="metric-value">${{node.metric_fan_out_dependency_graph}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="GDScript signal declarations in this file. Contextual only; they do not alter hotspot scores in this release.">Declarations</div><div class="metric-value">${{node.signal_count}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="Local signal emits. Contextual only; they do not alter hotspot scores in this release.">Emits</div><div class="metric-value">${{node.signal_emit_count}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="Local signal connections. Contextual only; they do not alter hotspot scores in this release.">Connections</div><div class="metric-value">${{node.signal_connection_count}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="Signal interactions whose ownership cannot be proven in this file. Contextual only; they do not alter hotspot scores in this release.">Unresolved</div><div class="metric-value">${{node.unresolved_signal_reference_count}}</div></div>
          <div class="metric-card" title="Number of functions and methods."><div class="metric-label">Functions</div><div class="metric-value">${{node.function_count}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="Complexity: estimated branching effort across this file. Higher values mean more paths to understand and test.">Complexity</div><div class="metric-value">${{node.metric_file_result_dependency_graph_louvain_modularity_in_file}}</div></div>
          <div class="metric-card"><div class="metric-label has-tooltip" data-tooltip="Hotspot: a structural priority signal combining size, complexity, and dependency coupling. Higher means a stronger refactoring candidate.">Hotspot</div><div class="metric-value" style="color:${{hotspotColor(node.metric_file_hotspot_score)}}">${{node.metric_file_hotspot_score}}</div></div>
        </div>
        ${{node.signal_declarations.length ? `<div class="node-path">Signals: ${{node.signal_declarations.join(', ')}}</div>` : ''}}
        ${{tags.length ? `<div class="node-tags">${{tags.join('')}}</div>` : ''}}
      `;
    }}

    function renderSignalFlow() {{
      const flow = payload.signal_flow;
      if (!flow || !flow.files) {{ signalFlowEl.innerHTML = '<div class="empty-state">No signal-flow data.</div>'; return; }}
      const active = flow.files.filter(f => f.declarations.length || f.signal_emit_count || f.signal_connection_count || f.unresolved_signal_reference_count)
        .sort((a,b) => ((b.signal_emit_count+b.signal_connection_count+b.declarations.length) - (a.signal_emit_count+a.signal_connection_count+a.declarations.length)) || a.file.localeCompare(b.file));
      const unresolved = flow.unresolved_references || [];
      signalFlowEl.innerHTML = active.length ? active.map(f => `<div class="fn-item"><div class="fn-body"><div class="fn-name">${{f.file}}</div><div class="fn-file">${{f.declarations.length}} declarations · ${{f.signal_emit_count}} emits · ${{f.signal_connection_count}} connections</div>${{f.declarations.length ? `<div class="node-path">${{f.declarations.join(', ')}}</div>` : ''}}</div>${{f.unresolved_signal_reference_count ? `<span class="fn-score fn-score-mid">${{f.unresolved_signal_reference_count}} unresolved targets</span>` : ''}}</div>`).join('') : '<div class="empty-state">No signal activity.</div>';
      if (unresolved.length) signalFlowEl.innerHTML += unresolved.slice(0, 10).map(r => `<div class="finding-item"><div class="finding-body"><div class="finding-msg">Unresolved signal target: ${{r.receiver || 'dynamic/local'}}.${{r.signal || 'signal'}}</div><div class="finding-file">${{r.file}}:${{r.line}} · ${{r.operation}} · receiver ownership cannot be proven statically</div></div></div>`).join('');
    }}

    function renderGodotInsights() {{
      const g = payload.godot_insights || {{}};
      const rows = [
        ['Autoloads', (g.autoloads || []).length, (g.autoloads || []).join(', ') || 'None declared in project.godot'],
        ['Input actions', (g.input_actions || []).length, `${{g.input_action_uses || 0}} static uses`],
        ['Groups', g.group_operations || 0, 'Membership, lookup, or broadcast operations'],
        ['RPC', (g.rpc_endpoints || 0) + (g.rpc_calls || 0), `${{g.rpc_endpoints || 0}} endpoints · ${{g.rpc_calls || 0}} calls`],
        ['Exports', g.exported_properties || 0, 'Serialized editor/resource contract fields'],
        ['Node bindings', g.node_bindings || 0, '$, get_node, or @onready references'],
        ['Lifecycle', g.lifecycle_callbacks || 0, 'Engine callbacks']
      ];
      godotInsightsEl.innerHTML = rows.map(([name, value, detail]) => `<div class="fn-item"><div class="fn-body"><div class="fn-name">${{name}}</div><div class="fn-file">${{detail}}</div></div><div class="fn-score fn-score-low">${{value}}</div></div>`).join('');
    }}

    // ── render: policy findings ────────────────────────────────────
    function renderPolicyFindings() {{
      const ps = payload.policy_summary;
      const parts = [];
      if (ps.critical_count) parts.push(`${{ps.critical_count}} critical`);
      if (ps.warning_count) parts.push(`${{ps.warning_count}} warning`);
      policyAside.textContent = parts.join(' · ');

      if (!payload.top_policy_findings || payload.top_policy_findings.length === 0) {{
        policyEl.innerHTML = '<div class="empty-state">No policy findings.</div>';
        return;
      }}
      policyEl.innerHTML = payload.top_policy_findings.map(f => `
        <div class="finding-item">
          <div class="finding-dot ${{f.severity}}"></div>
          <div class="finding-body">
            <div class="finding-msg">${{f.message}}</div>
            ${{f.file ? `<div class="finding-file">${{f.file}}</div>` : ''}}
          </div>
        </div>
      `).join('');
    }}

    // ── render: hot functions ──────────────────────────────────────
    function renderHotFunctions() {{
      if (!payload.top_functions || payload.top_functions.length === 0) {{
        hotFnEl.innerHTML = '<div class="empty-state">No function data.</div>';
        return;
      }}
      const maxScore = Math.max(...payload.top_functions.map(f => f.hotspot_score), 1);
      hotFnEl.innerHTML = payload.top_functions.map(f => {{
        const t = f.hotspot_score / maxScore;
        const cls = t > 0.65 ? 'fn-score-high' : t > 0.35 ? 'fn-score-mid' : 'fn-score-low';
        return `
          <div class="fn-item" title="${{f.file}}:${{f.start_line}} · ${{f.line_count}} lines · cyclomatic ${{f.cyclomatic_estimate}}">
            <div class="fn-body">
              <div class="fn-name">${{f.function_name}}</div>
              <div class="fn-file">${{f.file}}</div>
            </div>
            <div class="fn-score ${{cls}}">${{f.hotspot_score}}</div>
          </div>
        `;
      }}).join('');
    }}

    function renderInsights() {{
      const i = payload.insights;
      if (!i) {{
        gitInsightsEl.innerHTML = '<div class="empty-state">Available in the full health report.</div>';
        duplicateInsightsEl.innerHTML = '<div class="empty-state">Available in the full health report.</div>';
        refactorInsightsEl.innerHTML = '<div class="empty-state">Available in the full health report.</div>';
        return;
      }}
      gitInsightsEl.innerHTML = i.git_available
        ? [`<div class="fn-item"><span class="fn-name has-tooltip" data-tooltip="Bus factor is the smallest number of contributors responsible for 80% of non-merge commits. A value of 2 means two people account for most of the repository's history; losing either would concentrate knowledge risk.">Bus factor</span><span class="fn-score fn-score-mid">${{i.repo_bus_factor}}</span></div>`,
           `<div class="node-path">Churn score = Σ exp(−ln(2) × age days / configured half-life) × min((lines added + deleted)/100, 3). Files are highlighted when they are in the top churn quartile and changed in at least 3 commits in 90 days.</div>`,
           ...i.top_churn_files.map(f => `<div class="fn-item"><span class="fn-name" title="${{f.file}}">${{f.file}}</span><span class="fn-score fn-score-high">${{f.temporal_hotspot_score.toFixed(1)}}</span></div>`)]
            .join('')
        : '<div class="empty-state">Git signals unavailable.</div>';
      duplicateInsightsEl.innerHTML = i.duplicates.length
        ? i.duplicates.map(d => `<div class="finding-item"><div class="finding-body"><div class="finding-msg">${{d.file_a}}:${{d.start_line_a}} ↔ ${{d.file_b}}:${{d.start_line_b}}</div><div class="node-path">${{d.token_count}} normalized tokens</div></div></div>`).join('')
        : '<div class="empty-state">No duplicates detected.</div>';
      refactorInsightsEl.innerHTML = i.refactor_suggestions.length
        ? i.refactor_suggestions.map(s => `<div class="finding-item"><div class="finding-body"><div class="finding-msg">${{s}}</div></div></div>`).join('')
        : '<div class="empty-state">No concrete suggestion.</div>';
    }}

    // ── graph helpers ──────────────────────────────────────────────
    function getVisibleNodes() {{
      return nodes.filter(n => {{
        const inCluster = activeCluster == null || n.community_id === activeCluster;
        const haystack = (n.display_name + ' ' + n.absolute_name + ' ' + n.group).toLowerCase();
        const inSearch = !searchTerm || haystack.includes(searchTerm);
        return inCluster && inSearch;
      }});
    }}

    function buildNeighborSet(sel) {{
      if (!sel) return new Set();
      const nb = new Set([sel.id]);
      for (const lk of links) {{
        if (lk.source === sel.id) nb.add(lk.target);
        if (lk.target === sel.id) nb.add(lk.source);
      }}
      return nb;
    }}

    function hashVal(s) {{
      return Array.from(s).reduce((a,c) => ((a*31)+c.charCodeAt(0))>>>0, 7);
    }}

    function assignInitialLayout() {{
      const clusterMap = new Map();
      for (const n of nodes) {{
        const m = clusterMap.get(n.community_id) || [];
        m.push(n);
        clusterMap.set(n.community_id, m);
      }}
      const comms = [...clusterMap.keys()].sort((a,b) => a-b);
      const cols = Math.max(1, Math.ceil(Math.sqrt(comms.length)));
      const cx = W/2, cy = H/2;
      comms.forEach((cid, ci) => {{
        const members = [...clusterMap.get(cid)].sort((a,b) =>
          (b.metric_fan_in_dependency_graph - a.metric_fan_in_dependency_graph) ||
          a.absolute_name.localeCompare(b.absolute_name)
        );
        const row = Math.floor(ci/cols), col = ci%cols;
        const angle = (ci/Math.max(1,comms.length))*Math.PI*2;
        const rx = 280, ry = 210;
        const ax = cx + (col-(cols-1)/2)*200 + Math.cos(angle)*Math.min(80, rx/Math.max(1,comms.length/6));
        const ay = cy + (row-(Math.ceil(comms.length/cols)-1)/2)*170 + Math.sin(angle)*Math.min(60, ry/Math.max(1,comms.length/6));
        members.forEach((n, i) => {{
          const g = i*2.399963229728653;
          const r = 16 + Math.sqrt(i+1)*20 + hashVal(n.id)%7;
          n.x = Math.min(W-28, Math.max(28, ax+Math.cos(g)*r));
          n.y = Math.min(H-28, Math.max(28, ay+Math.sin(g)*r));
          n.vx = 0; n.vy = 0;
        }});
      }});
    }}

    function stabilizeLayout(iters) {{
      const ideal = 82;
      const cx = W/2, cy = H/2;
      for (let iter = 0; iter < iters; iter++) {{
        const alpha = 1 - iter/iters;
        const anchors = new Map();
        for (const n of nodes) {{
          const a = anchors.get(n.community_id) || {{x:0,y:0,c:0}};
          a.x+=n.x; a.y+=n.y; a.c++;
          anchors.set(n.community_id, a);
        }}
        for (const a of anchors.values()) {{ a.x/=a.c; a.y/=a.c; }}
        for (const n of nodes) {{
          let fx=0, fy=0;
          for (const o of nodes) {{
            if (n===o) continue;
            const dx=n.x-o.x, dy=n.y-o.y;
            const dsq = Math.max(36, dx*dx+dy*dy);
            const d = Math.sqrt(dsq);
            const rep = (320*(0.3+alpha*0.85))/dsq;
            fx += (dx/d)*rep; fy += (dy/d)*rep;
          }}
          const a = anchors.get(n.community_id);
          fx += (a.x-n.x)*(0.012+alpha*0.018);
          fy += (a.y-n.y)*(0.012+alpha*0.018);
          fx += (cx-n.x)*0.001; fy += (cy-n.y)*0.001;
          n.vx = (n.vx+fx)*0.78; n.vy = (n.vy+fy)*0.78;
        }}
        for (const lk of links) {{
          const dx=lk.targetNode.x-lk.sourceNode.x, dy=lk.targetNode.y-lk.sourceNode.y;
          const d=Math.max(1,Math.sqrt(dx*dx+dy*dy));
          const sp=(d-ideal)*(0.018+alpha*0.014);
          const fx=(dx/d)*sp, fy=(dy/d)*sp;
          lk.sourceNode.vx+=fx; lk.sourceNode.vy+=fy;
          lk.targetNode.vx-=fx; lk.targetNode.vy-=fy;
        }}
        for (const n of nodes) {{
          n.x = Math.min(W-22, Math.max(22, n.x+n.vx));
          n.y = Math.min(H-22, Math.max(22, n.y+n.vy));
        }}
      }}
      for (const n of nodes) {{ n.vx=0; n.vy=0; }}
    }}

    // ── draw ───────────────────────────────────────────────────────
    function draw() {{
      const visIds = new Set(getVisibleNodes().map(n => n.id));
      const selNode = activeNodeId ? nodeMap.get(activeNodeId) : null;
      const nb = buildNeighborSet(selNode);
      const visLinks = visibleLinks().filter(lk => visIds.has(lk.source) && visIds.has(lk.target));

      linkLayer.innerHTML = visLinks.map(lk => {{
        const isActive = !!(selNode && (lk.source===selNode.id||lk.target===selNode.id));
        const sameCom = lk.sourceNode.community_id === lk.targetNode.community_id;
        let stroke, sw, so;
        if (isActive) {{
          stroke = colorMode==='hotspot' ? '#D9813A' : communityColor(lk.sourceNode.community_id);
          sw = 2.5; so = 1;
        }} else if (lk.cross_group) {{
          stroke = 'var(--edge-cross)'; sw = 1; so = selNode && !isActive ? 0.06 : 0.55;
        }} else if (sameCom && colorMode==='community') {{
          stroke = communityColor(lk.sourceNode.community_id); sw = 1; so = selNode && !isActive ? 0.06 : 0.35;
        }} else {{
          stroke = 'var(--edge-default)'; sw = 1; so = selNode && !isActive ? 0.04 : 0.18;
        }}
        return `<line x1="${{lk.sourceNode.x.toFixed(1)}}" y1="${{lk.sourceNode.y.toFixed(1)}}" x2="${{lk.targetNode.x.toFixed(1)}}" y2="${{lk.targetNode.y.toFixed(1)}}" stroke="${{stroke}}" stroke-width="${{sw}}" stroke-opacity="${{so}}"/>`;
      }}).join('');

      nodeLayer.innerHTML = getVisibleNodes().map(n => {{
        const isActive = activeNodeId===n.id;
        const visible = !selNode || nb.has(n.id);
        const fill = nodeColor(n);
        const r = 5.5 + Math.sqrt(n.metric_fan_in_dependency_graph+n.metric_fan_out_dependency_graph+1)*1.9;
        const showLabel = isActive || (selNode && visible) ||
          (!selNode && colorMode==='hotspot' && n.metric_file_hotspot_score > maxHotspot*0.38) ||
          (!selNode && colorMode==='community' && n.metric_fan_in_dependency_graph+n.metric_fan_out_dependency_graph >= 3);
        const fo = visible ? 1 : 0.12;
        let extras = '';
        if (n.has_cycle) {{
          extras += `<circle r="${{(r+4).toFixed(1)}}" fill="none" stroke="${{fill}}" stroke-width="1" stroke-dasharray="3 2" stroke-opacity="${{visible ? 0.65 : 0.08}}"/>`;
        }}
        return `
          <g data-nid="${{n.id}}" transform="translate(${{n.x.toFixed(1)}} ${{n.y.toFixed(1)}})">
            ${{extras}}
            <circle r="${{r.toFixed(1)}}" fill="${{fill}}" fill-opacity="${{fo*0.92}}" stroke="${{isActive ? '#fff' : 'rgba(255,255,255,0.18)'}}" stroke-width="${{isActive ? 2 : 1}}" stroke-opacity="${{fo}}"/>
            <text y="${{-(r+7)}}" text-anchor="middle" font-size="10.5" font-family="-apple-system,BlinkMacSystemFont,'Inter',sans-serif" fill="${{isActive ? '#fff' : 'var(--text-2)'}}" fill-opacity="${{showLabel ? fo : 0}}">${{n.display_name}}</text>
          </g>
        `;
      }}).join('');

      nodeLayer.querySelectorAll('[data-nid]').forEach(g => {{
        g.style.cursor = 'pointer';
        g.addEventListener('mouseenter', () => {{
          updateSelection(nodeMap.get(g.dataset.nid));
        }});
        g.addEventListener('mouseleave', () => {{
          updateSelection(activeNodeId ? nodeMap.get(activeNodeId) : null);
        }});
        g.addEventListener('mousedown', e => {{
          draggingNode = nodeMap.get(g.dataset.nid);
          e.preventDefault();
        }});
        g.addEventListener('click', e => {{
          e.stopPropagation();
          const n = nodeMap.get(g.dataset.nid);
          activeNodeId = activeNodeId===n.id ? null : n.id;
          activeCluster = null;
          renderClusters();
          updateSelection(activeNodeId ? nodeMap.get(activeNodeId) : null);
          draw();
        }});
      }});
    }}

    // ── pan / drag ─────────────────────────────────────────────────
    svg.addEventListener('mousemove', e => {{
      if (!draggingNode) return;
      const rect = svg.getBoundingClientRect();
      const viewBox = svg.viewBox.baseVal;
      const scale = Math.min(rect.width / viewBox.width, rect.height / viewBox.height);
      const offsetX = (rect.width - viewBox.width * scale) / 2;
      const offsetY = (rect.height - viewBox.height * scale) / 2;
      draggingNode.x = viewBox.x + (e.clientX - rect.left - offsetX) / scale;
      draggingNode.y = viewBox.y + (e.clientY - rect.top - offsetY) / scale;
      draggingNode.vx=0; draggingNode.vy=0;
      draw();
    }});
    svg.addEventListener('click', e => {{
      if (e.target===svg||e.target.tagName==='svg') {{
        activeNodeId=null; activeCluster=null;
        renderClusters(); updateSelection(null); draw();
      }}
    }});
    window.addEventListener('mouseup', () => {{ draggingNode=null; }});
    document.getElementById('zoom-in').addEventListener('click', () => {{
      zoom = Math.min(3, zoom * 1.25);
      const selected = activeNodeId ? nodeMap.get(activeNodeId) : null;
      if (selected) zoomCenter = {{ x: selected.x, y: selected.y }};
      applyZoom();
    }});
    document.getElementById('zoom-out').addEventListener('click', () => {{
      zoom = Math.max(1, zoom / 1.25);
      zoomCenter = {{ x: (zoomCenter.x + W / 2) / 2, y: (zoomCenter.y + H / 2) / 2 }};
      if (zoom === 1) zoomCenter = {{ x: W / 2, y: H / 2 }};
      applyZoom();
    }});
    document.getElementById('zoom-reset').addEventListener('click', () => {{ zoom = 1; zoomCenter = {{ x: W / 2, y: H / 2 }}; applyZoom(); }});
    document.addEventListener('keydown', e => {{
      if (e.key==='Escape') {{
        activeNodeId=null; activeCluster=null;
        renderClusters(); updateSelection(null); draw();
      }}
    }});

    // ── search ─────────────────────────────────────────────────────
    searchEl.addEventListener('input', () => {{
      searchTerm = searchEl.value.trim().toLowerCase();
      draw();
    }});

    // ── color mode ─────────────────────────────────────────────────
    document.getElementById('mode-community').addEventListener('click', () => {{
      colorMode='community';
      document.getElementById('mode-community').classList.add('active');
      document.getElementById('mode-hotspot').classList.remove('active');
      hotspotLegend.classList.add('hidden');
      draw();
    }});
    document.getElementById('mode-hotspot').addEventListener('click', () => {{
      colorMode='hotspot';
      document.getElementById('mode-hotspot').classList.add('active');
      document.getElementById('mode-community').classList.remove('active');
      hotspotLegend.classList.remove('hidden');
      draw();
    }});

    // ── activate graph ─────────────────────────────────────────────
    function activateGraph() {{
      graphPayload = payload.graphs.file_result_dependency_graph;
      nodes = graphPayload.graph.nodes.map((n,i) => ({{...n,index:i}}));
      nodeMap = new Map(nodes.map(n => [n.id,n]));
      links = graphPayload.graph.links.map(lk => ({{
        ...lk,
        sourceNode: nodeMap.get(lk.source),
        targetNode: nodeMap.get(lk.target)
      }})).filter(lk => lk.sourceNode && lk.targetNode);
      clusters = Object.values(graphPayload.cluster_metrics_map).sort((a,b)=>a.community_id-b.community_id);
      stats = graphPayload.statistics;
      maxHotspot = Math.max(...nodes.map(n=>n.metric_file_hotspot_score), 1);
      activeKinds = new Set(links.map(lk => lk.kind));
      activeCluster=null; activeNodeId=null; draggingNode=null;
      selEl.className='empty-state';
      selEl.textContent='Click a node or cluster to inspect.';
      assignInitialLayout();
      stabilizeLayout(160);
      renderRelationFilters();
      renderStats();
      renderClusters();
      draw();
    }}

    // ── boot ───────────────────────────────────────────────────────
    renderPolicyFindings();
    renderHotFunctions();
    renderInsights();
    renderSignalFlow();
    renderGodotInsights();
    applyZoom();
    activateGraph();
  </script>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::{build_export, render_emerge_json, render_html};
    use crate::model::{
        DependencyEdge, DependencyKind, EntityMetric, EntityNode, EntityScanReport, FileMetric,
        GraphAggregateMetrics, GraphCommunity, GraphReport, MetricsReport, OutputMetadata,
        ProjectFile,
    };

    #[test]
    fn export_contains_cluster_metrics_and_node_metrics() {
        let graph = GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: 3,
            files: vec![
                ProjectFile {
                    id: "a.gd".to_string(),
                    path: "a.gd".to_string(),
                    group: Some("core".to_string()),
                    line_count: 20,
                },
                ProjectFile {
                    id: "b.gd".to_string(),
                    path: "b.gd".to_string(),
                    group: Some("core".to_string()),
                    line_count: 15,
                },
                ProjectFile {
                    id: "c.gd".to_string(),
                    path: "c.gd".to_string(),
                    group: Some("ui".to_string()),
                    line_count: 10,
                },
            ],
            edges: vec![
                DependencyEdge {
                    from: "a.gd".to_string(),
                    to: "b.gd".to_string(),
                    kind: DependencyKind::Preload,
                    line: 3,
                },
                DependencyEdge {
                    from: "b.gd".to_string(),
                    to: "a.gd".to_string(),
                    kind: DependencyKind::Load,
                    line: 8,
                },
            ],
            cycles: vec![vec!["a.gd".to_string(), "b.gd".to_string()]],
            parser_warnings: vec![
                "c.gd:2: unresolved resource path `res://missing.gd`".to_string(),
            ],
            file_communities: vec![
                GraphCommunity {
                    id: "a.gd".to_string(),
                    community_id: 0,
                },
                GraphCommunity {
                    id: "b.gd".to_string(),
                    community_id: 0,
                },
                GraphCommunity {
                    id: "c.gd".to_string(),
                    community_id: 1,
                },
            ],
            file_graph_metrics: Some(GraphAggregateMetrics {
                louvain_modularity: 0.25,
                community_count: 2,
                total_sloc: 45,
                group_count: 2,
            }),
            entity_scan: Some(EntityScanReport {
                entities: vec![
                    EntityNode {
                        id: "A".to_string(),
                        display_name: "A".to_string(),
                        file: "a.gd".to_string(),
                        class_name: Some("A".to_string()),
                        extends: None,
                        start_line: 1,
                        end_line: 20,
                        line_count: 20,
                        method_count: 2,
                    },
                    EntityNode {
                        id: "B".to_string(),
                        display_name: "B".to_string(),
                        file: "b.gd".to_string(),
                        class_name: Some("B".to_string()),
                        extends: Some("A".to_string()),
                        start_line: 1,
                        end_line: 15,
                        line_count: 15,
                        method_count: 1,
                    },
                ],
                dependency_edges: vec![crate::model::EntityEdge {
                    from: "A".to_string(),
                    to: "B".to_string(),
                    kind: DependencyKind::Preload,
                    line: 3,
                }],
                inheritance_edges: vec![crate::model::EntityEdge {
                    from: "B".to_string(),
                    to: "A".to_string(),
                    kind: DependencyKind::Extends,
                    line: 1,
                }],
                complete_edges: vec![
                    crate::model::EntityEdge {
                        from: "A".to_string(),
                        to: "B".to_string(),
                        kind: DependencyKind::Preload,
                        line: 3,
                    },
                    crate::model::EntityEdge {
                        from: "B".to_string(),
                        to: "A".to_string(),
                        kind: DependencyKind::Extends,
                        line: 1,
                    },
                ],
                communities: vec![
                    GraphCommunity {
                        id: "A".to_string(),
                        community_id: 0,
                    },
                    GraphCommunity {
                        id: "B".to_string(),
                        community_id: 0,
                    },
                ],
                complete_graph_metrics: GraphAggregateMetrics {
                    louvain_modularity: 0.0,
                    community_count: 1,
                    total_sloc: 35,
                    group_count: 0,
                },
            }),
        };
        let metrics = MetricsReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            files: vec![
                FileMetric {
                    file: "a.gd".to_string(),
                    line_count: 20,
                    function_count: 2,
                    number_of_methods: 2,
                    signal_count: 0,
                    signal_emit_count: 0,
                    signal_connection_count: 0,
                    unresolved_signal_reference_count: 0,
                    public_function_count: 1,
                    outgoing_dependency_count: 1,
                    incoming_dependency_count: 1,
                    cycle_count: 1,
                    ws_complexity: 8,
                    tfidf: 0.7,
                    louvain_modularity: 0,
                    file_hotspot_score: 14,
                },
                FileMetric {
                    file: "b.gd".to_string(),
                    line_count: 15,
                    function_count: 1,
                    number_of_methods: 1,
                    signal_count: 1,
                    signal_emit_count: 0,
                    signal_connection_count: 0,
                    unresolved_signal_reference_count: 0,
                    public_function_count: 1,
                    outgoing_dependency_count: 1,
                    incoming_dependency_count: 1,
                    cycle_count: 1,
                    ws_complexity: 4,
                    tfidf: 0.6,
                    louvain_modularity: 0,
                    file_hotspot_score: 12,
                },
                FileMetric {
                    file: "c.gd".to_string(),
                    line_count: 10,
                    function_count: 1,
                    number_of_methods: 1,
                    signal_count: 0,
                    signal_emit_count: 0,
                    signal_connection_count: 0,
                    unresolved_signal_reference_count: 0,
                    public_function_count: 1,
                    outgoing_dependency_count: 0,
                    incoming_dependency_count: 0,
                    cycle_count: 0,
                    ws_complexity: 2,
                    tfidf: 0.4,
                    louvain_modularity: 1,
                    file_hotspot_score: 5,
                },
            ],
            functions: Vec::new(),
            entities: vec![
                EntityMetric {
                    entity: "A".to_string(),
                    file: "a.gd".to_string(),
                    line_count: 20,
                    method_count: 2,
                    signal_count: 0,
                    signal_emit_count: 0,
                    signal_connection_count: 0,
                    unresolved_signal_reference_count: 0,
                    ws_complexity: 8,
                    tfidf: 0.7,
                    incoming_dependency_count: 0,
                    outgoing_dependency_count: 1,
                    incoming_inheritance_count: 1,
                    outgoing_inheritance_count: 0,
                    louvain_modularity: 0,
                },
                EntityMetric {
                    entity: "B".to_string(),
                    file: "b.gd".to_string(),
                    line_count: 15,
                    method_count: 1,
                    signal_count: 1,
                    signal_emit_count: 0,
                    signal_connection_count: 0,
                    unresolved_signal_reference_count: 0,
                    ws_complexity: 4,
                    tfidf: 0.6,
                    incoming_dependency_count: 1,
                    outgoing_dependency_count: 0,
                    incoming_inheritance_count: 0,
                    outgoing_inheritance_count: 1,
                    louvain_modularity: 0,
                },
            ],
            signal_flow: crate::model::SignalFlowReport {
                available: true,
                files: Vec::new(),
                unresolved_references: Vec::new(),
            },
            duplication: crate::model::DuplicationReport {
                available: true,
                unavailable_reason: None,
                clones: Vec::new(),
            },
        };

        let export = build_export(&graph, &metrics, &[], 0);
        let json = render_emerge_json(&graph, &metrics, &[], 0).expect("render export");

        assert_eq!(
            export
                .graphs
                .file_result_dependency_graph
                .statistics
                .node_count,
            3
        );
        assert!(
            export
                .graphs
                .file_result_dependency_graph
                .cluster_metrics_map
                .contains_key("0")
        );
        assert!(json.contains("\"metric_fan_in_dependency_graph\""));
        assert!(json.contains("\"cluster_metrics_map\""));
        assert!(json.contains("\"policy_summary\""));
        assert!(json.contains("\"top_functions\""));
        assert!(json.contains("\"signal_flow\""));
        let html = render_html(&json, "fixture");
        assert!(html.contains("id=\"gdhealth-tooltip\""));
        assert!(html.contains("Signal activity"));
        assert!(html.contains("Unresolved signal target"));
    }
}
