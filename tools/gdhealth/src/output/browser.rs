use crate::error::Result;
use crate::fs;
use crate::model::{
    DependencyKind, EntityEdge, EntityMetric, GraphReport, MetricsReport, TOOL_NAME, TOOL_VERSION,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
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

pub fn render_emerge_json(graph: &GraphReport, metrics: &MetricsReport) -> Result<String> {
    let export = build_export(graph, metrics);
    Ok(serde_json::to_string_pretty(&export)?)
}

pub fn emit_emerge_html(
    root: &Path,
    output: Option<&Path>,
    graph: &GraphReport,
    metrics: &MetricsReport,
) -> Result<()> {
    let export = build_export(graph, metrics);
    let json = serde_json::to_string_pretty(&export)?;
    let (html_path, json_path) = resolve_html_output_paths(root, output);
    let html = render_html(&json, &export.metadata.project_name);

    fs::write_output(&html_path, &html)?;
    fs::write_output(&json_path, &json)?;
    Ok(())
}

fn build_export(graph: &GraphReport, metrics: &MetricsReport) -> BrowserExport {
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
        .filter(|edge| node_id_set.contains(edge.from.as_str()) && node_id_set.contains(edge.to.as_str()))
        .map(|edge| BrowserLink {
            source: edge.from.clone(),
            target: edge.to.clone(),
            kind: dependency_kind_label(&edge.kind).to_string(),
            line: edge.line,
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
    let entity_dependency_export = build_entity_graph_export(
        "entity_result_dependency_graph",
        &entity_scan.entities,
        &entity_scan.dependency_edges,
        &entity_metric_by_id,
        &entity_scan.communities,
        entity_scan.complete_graph_metrics.total_sloc,
    );
    let entity_inheritance_export = build_entity_graph_export(
        "entity_result_inheritance_graph",
        &entity_scan.entities,
        &entity_scan.inheritance_edges,
        &entity_metric_by_id,
        &entity_scan.communities,
        entity_scan.complete_graph_metrics.total_sloc,
    );
    let entity_complete_export = build_entity_graph_export(
        "entity_result_complete_graph",
        &entity_scan.entities,
        &entity_scan.complete_edges,
        &entity_metric_by_id,
        &entity_scan.communities,
        entity_scan.complete_graph_metrics.total_sloc,
    );

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
    }
}

fn build_entity_graph_export(
    graph_name: &'static str,
    entities: &[crate::model::EntityNode],
    edges: &[EntityEdge],
    entity_metric_by_id: &BTreeMap<&str, &EntityMetric>,
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
                signal_count: 0,
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
        .filter(|edge| entity_id_set.contains(edge.from.as_str()) && entity_id_set.contains(edge.to.as_str()))
        .map(|edge| BrowserLink {
            source: edge.from.clone(),
            target: edge.to.clone(),
            kind: dependency_kind_label(&edge.kind).to_string(),
            line: edge.line,
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
  <title>{project_name} Graph Explorer</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #f4efe7;
      --panel: rgba(255, 250, 243, 0.94);
      --ink: #1f1b18;
      --muted: #6d6258;
      --accent: #0d6b6f;
      --accent-2: #d1643f;
      --grid: rgba(31, 27, 24, 0.08);
      --edge: rgba(31, 27, 24, 0.18);
      --border: rgba(31, 27, 24, 0.12);
      --shadow: 0 18px 40px rgba(34, 24, 18, 0.12);
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      min-height: 100vh;
      font-family: Georgia, "Times New Roman", serif;
      background:
        radial-gradient(circle at top left, rgba(209, 100, 63, 0.18), transparent 32%),
        radial-gradient(circle at right, rgba(13, 107, 111, 0.16), transparent 24%),
        linear-gradient(180deg, #f8f4ed 0%, var(--bg) 100%);
      color: var(--ink);
    }}
    .app {{
      display: grid;
      grid-template-columns: 320px minmax(0, 1fr) 320px;
      min-height: 100vh;
      gap: 16px;
      padding: 16px;
    }}
    .panel {{
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 20px;
      box-shadow: var(--shadow);
      backdrop-filter: blur(12px);
      overflow: hidden;
    }}
    .panel-inner {{
      padding: 18px 18px 20px;
      display: flex;
      flex-direction: column;
      gap: 16px;
      height: 100%;
    }}
    h1, h2, h3, p {{ margin: 0; }}
    h1 {{ font-size: 1.55rem; line-height: 1.1; }}
    h2 {{ font-size: 0.92rem; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); }}
    .hero {{
      display: flex;
      flex-direction: column;
      gap: 8px;
    }}
    .hero p {{ color: var(--muted); line-height: 1.4; }}
    .search {{
      width: 100%;
      border: 1px solid var(--border);
      border-radius: 999px;
      padding: 12px 14px;
      font: inherit;
      background: rgba(255, 255, 255, 0.8);
      color: var(--ink);
    }}
    .graph-select {{
      width: 100%;
      border: 1px solid var(--border);
      border-radius: 14px;
      padding: 12px 14px;
      font: inherit;
      background: rgba(255, 255, 255, 0.8);
      color: var(--ink);
    }}
    .stats {{
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
    }}
    .stat {{
      padding: 12px;
      border-radius: 16px;
      background: rgba(13, 107, 111, 0.08);
      border: 1px solid rgba(13, 107, 111, 0.12);
    }}
    .stat strong {{
      display: block;
      font-size: 1.3rem;
    }}
    [title] {{ cursor: help; }}
    .clusters, .files {{
      display: flex;
      flex-direction: column;
      gap: 8px;
      min-height: 0;
    }}
    .scroll {{
      overflow: auto;
      max-height: 34vh;
      padding-right: 4px;
    }}
    .cluster-item, .file-item {{
      border: 1px solid var(--border);
      border-radius: 14px;
      padding: 10px 12px;
      background: rgba(255, 255, 255, 0.62);
      cursor: pointer;
    }}
    .cluster-item.active, .file-item.active {{
      border-color: var(--accent);
      box-shadow: inset 0 0 0 1px var(--accent);
    }}
    .cluster-title {{
      display: flex;
      align-items: center;
      gap: 8px;
      font-weight: 700;
    }}
    .swatch {{
      width: 12px;
      height: 12px;
      border-radius: 999px;
      flex: none;
    }}
    .meta {{
      color: var(--muted);
      font-size: 0.92rem;
      line-height: 1.4;
    }}
    .canvas-panel {{
      position: relative;
      min-height: 72vh;
      overflow: hidden;
    }}
    .canvas-panel::before {{
      content: "";
      position: absolute;
      inset: 0;
      background-image:
        linear-gradient(var(--grid) 1px, transparent 1px),
        linear-gradient(90deg, var(--grid) 1px, transparent 1px);
      background-size: 28px 28px;
      mask-image: linear-gradient(180deg, rgba(0,0,0,0.75), transparent);
      pointer-events: none;
    }}
    svg {{
      width: 100%;
      height: 100%;
      display: block;
      position: relative;
      z-index: 1;
    }}
    .legend {{
      position: absolute;
      left: 18px;
      bottom: 18px;
      z-index: 2;
      background: rgba(255, 250, 243, 0.88);
      border: 1px solid var(--border);
      border-radius: 14px;
      padding: 10px 12px;
      color: var(--muted);
      font-size: 0.9rem;
    }}
    .detail-grid {{
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
    }}
    .detail-card {{
      padding: 10px 12px;
      border-radius: 14px;
      background: rgba(209, 100, 63, 0.08);
      border: 1px solid rgba(209, 100, 63, 0.12);
    }}
    .detail-card strong {{
      display: block;
      font-size: 1.1rem;
    }}
    .empty {{
      color: var(--muted);
      line-height: 1.4;
    }}
    @media (max-width: 1180px) {{
      .app {{
        grid-template-columns: 1fr;
      }}
      .canvas-panel {{
        min-height: 58vh;
      }}
      .scroll {{
        max-height: none;
      }}
    }}
  </style>
</head>
<body>
  <div class="app">
    <aside class="panel">
      <div class="panel-inner">
        <div class="hero">
          <h2>gdhealth browser export</h2>
          <h1>{project_name}</h1>
          <p>Force-directed dependency view with deterministic cluster metrics.</p>
        </div>
        <select id="graph-type" class="graph-select"></select>
        <input id="search" class="search" type="search" placeholder="Search files or groups">
        <section>
          <h2>Graph Stats</h2>
          <div id="stats" class="stats"></div>
        </section>
        <section class="clusters">
          <h2>Clusters</h2>
          <div id="clusters" class="scroll"></div>
        </section>
      </div>
    </aside>
    <main class="panel canvas-panel">
      <svg id="graph" viewBox="0 0 960 720" preserveAspectRatio="xMidYMid meet"></svg>
      <div class="legend">Drag nodes to inspect neighborhoods. Click a cluster card to isolate it.</div>
    </main>
    <aside class="panel">
      <div class="panel-inner">
        <section>
          <h2>Selection</h2>
          <div id="selection" class="empty">Hover or click a node to inspect file metrics.</div>
        </section>
        <section class="files">
          <h2>Visible Nodes</h2>
          <div id="files" class="scroll"></div>
        </section>
      </div>
    </aside>
  </div>
  <script id="graph-data" type="application/json">{data_json}</script>
  <script>
    const payload = JSON.parse(document.getElementById('graph-data').textContent);
    const graphTypeLabels = {{
      file_result_dependency_graph: 'File dependencies',
      entity_result_dependency_graph: 'Entity dependencies',
      entity_result_inheritance_graph: 'Entity inheritance',
      entity_result_complete_graph: 'Entity complete'
    }};
    const selectionEl = document.getElementById('selection');
    const clustersEl = document.getElementById('clusters');
    const filesEl = document.getElementById('files');
    const searchEl = document.getElementById('search');
    const graphTypeEl = document.getElementById('graph-type');
    const svg = document.getElementById('graph');
    const statsEl = document.getElementById('stats');
    const width = 960;
    const height = 720;
    let graphType = payload.analysis_config.default_graph_type;
    let graphPayload = null;
    let nodes = [];
    let nodeMap = new Map();
    let links = [];
    let clusters = [];
    let stats = null;
    let activeCluster = null;
    let searchTerm = '';
    let activeNodeId = null;

    function colorForCommunity(communityId) {{
      const hue = (communityId * 57) % 360;
      return `hsl(${{hue}} 62% 46%)`;
    }}

    function formatNumber(value) {{
      return Number.isInteger(value) ? String(value) : value.toFixed(2);
    }}

    function renderStats() {{
      const cards = [
        ['Nodes', stats.node_count, '.gd files analyzed in this graph.'],
        ['Edges', stats.edge_count, 'Dependency connections (preload / load / extends) between files. Higher = more coupled.'],
        ['Clusters', stats.community_count, 'File groups detected by Louvain community detection. Ideal: a handful of cohesive clusters. Many tiny clusters = low cohesion.'],
        ['SLOC', stats.total_sloc, 'Source Lines of Code across all files (includes blanks and comments).'],
      ];
      statsEl.innerHTML = cards.map(([label, value, tip]) => `
        <div class="stat" title="${{tip}}">
          <span class="meta">${{label}}</span>
          <strong>${{value}}</strong>
        </div>
      `).join('');
    }}

    function renderClusters() {{
      clustersEl.innerHTML = clusters.map(cluster => `
        <button class="cluster-item${{activeCluster === cluster.community_id ? ' active' : ''}}" data-community="${{cluster.community_id}}">
          <div class="cluster-title">
            <span class="swatch" style="background:${{colorForCommunity(cluster.community_id)}}"></span>
            ${{cluster.label || `Cluster ${{cluster.community_id}}`}}
          </div>
          <div class="meta">
            <span title="Number of .gd files grouped in this cluster by community detection.">${{cluster.node_count}} files</span>,
            <span title="Source Lines of Code total. Ideal: balanced across clusters. Clusters over 500 SLOC are good refactor candidates.">${{cluster.total_sloc}} SLOC</span>,
            <span title="Average fan-in: how many OTHER files import files from this cluster. High = stable API used by many, but risky to change. Ideal avg 1-4. Over 8 = consider extracting a stable interface.">fan-in ${{formatNumber(cluster.avg_fan_in)}}</span> /
            <span title="Average fan-out: how many files each file in this cluster imports on average. High = too many responsibilities. Ideal avg 0-3. Over 6 = consider splitting.">fan-out ${{formatNumber(cluster.avg_fan_out)}}</span>
          </div>
        </button>
      `).join('');

      clustersEl.querySelectorAll('[data-community]').forEach(button => {{
        button.addEventListener('click', () => {{
          const communityId = Number(button.getAttribute('data-community'));
          activeCluster = activeCluster === communityId ? null : communityId;
          renderClusters();
          renderFiles();
          updateSelection(activeNodeId ? nodeMap.get(activeNodeId) : null);
          draw();
        }});
      }});
    }}

    function renderFiles() {{
      const visible = getVisibleNodes();
      filesEl.innerHTML = visible.map(node => `
        <button class="file-item${{activeNodeId === node.id ? ' active' : ''}}" data-node-id="${{node.id}}">
          <div><strong>${{node.display_name}}</strong></div>
          <div class="meta">${{node.absolute_name}}</div>
        </button>
      `).join('');
      filesEl.querySelectorAll('[data-node-id]').forEach(button => {{
        button.addEventListener('click', () => {{
          const node = nodeMap.get(button.getAttribute('data-node-id'));
          activeNodeId = node.id;
          updateSelection(node);
          renderFiles();
          draw();
        }});
      }});
    }}

    function updateSelection(node) {{
      if (!node) {{
        if (activeCluster == null) {{
          selectionEl.className = 'empty';
          selectionEl.textContent = 'Hover or click a node to inspect file metrics.';
          return;
        }}
        const cluster = clusters.find(item => item.community_id === activeCluster);
        selectionEl.className = '';
        selectionEl.innerHTML = `
          <h3>${{cluster.label || `Cluster ${{cluster.community_id}}`}}</h3>
          <p class="meta">${{cluster.node_count}} files, ${{cluster.total_sloc}} SLOC, ${{cluster.percent_total_sloc.toFixed(1)}}% of project SLOC</p>
          <div class="detail-grid">
            <div class="detail-card" title="Average fan-in: how many files import from this cluster per file. High = widely used, but costly to change. Ideal 1-4. Over 8 = extract a stable interface."><span class="meta">Avg fan-in</span><strong>${{formatNumber(cluster.avg_fan_in)}}</strong></div>
            <div class="detail-card" title="Average fan-out: how many files each file here imports. High = many responsibilities. Ideal 0-3. Over 6 = split the cluster."><span class="meta">Avg fan-out</span><strong>${{formatNumber(cluster.avg_fan_out)}}</strong></div>
            <div class="detail-card" title="Average number of functions per file. Ideal 5-15. Over 25 per file = consider splitting."><span class="meta">Avg functions</span><strong>${{formatNumber(cluster.avg_function_count)}}</strong></div>
            <div class="detail-card" title="Total .gd files in this cluster."><span class="meta">Files</span><strong>${{cluster.node_count}}</strong></div>
          </div>
          <p class="meta">${{cluster.files.slice(0, 8).join('<br>')}}</p>
        `;
        return;
      }}

      selectionEl.className = '';
      selectionEl.innerHTML = `
        <h3>${{node.display_name}}</h3>
        <p class="meta">${{node.absolute_name}}</p>
        <div class="detail-grid">
          <div class="detail-card" title="Total lines in this file (including blank lines and comments)."><span class="meta">Lines</span><strong>${{node.line_count}}</strong></div>
          <div class="detail-card" title="Fan-in: how many other files import or preload this one. High = many dependents, risky to modify. Ideal 0-3. Over 8 = consider extracting a stable interface or autoload."><span class="meta">Fan-in</span><strong>${{node.metric_fan_in_dependency_graph}}</strong></div>
          <div class="detail-card" title="Fan-out: how many files this script imports or preloads. High = too many responsibilities. Ideal 0-4. Over 8 = split this file."><span class="meta">Fan-out</span><strong>${{node.metric_fan_out_dependency_graph}}</strong></div>
          <div class="detail-card" title="Louvain cluster ID. Files with the same ID are tightly related by dependency patterns and were grouped together automatically."><span class="meta">Community</span><strong>${{node.community_id}}</strong></div>
          <div class="detail-card" title="Number of functions and methods in this file. Ideal 5-15. Over 25 = consider splitting into smaller scripts or using composition."><span class="meta">Functions</span><strong>${{node.function_count}}</strong></div>
          <div class="detail-card" title="Hotspot score: weighted signal combining dependencies, size, and cycle membership. Higher = more likely to cause bugs when changed. Ideal under 10. Over 20 = refactor priority."><span class="meta">Hotspot</span><strong>${{node.metric_file_hotspot_score}}</strong></div>
        </div>
        <p class="meta">Group: ${{node.group}} | Parser warnings: ${{node.parser_warning_count}} | Cycle member: ${{node.has_cycle ? 'yes' : 'no'}}</p>
      `;
    }}

    function getVisibleNodes() {{
      return nodes.filter(node => {{
        const matchesCluster = activeCluster == null || node.community_id === activeCluster;
        const haystack = `${{node.display_name}} ${{node.absolute_name}} ${{node.group}}`.toLowerCase();
        const matchesSearch = !searchTerm || haystack.includes(searchTerm);
        return matchesCluster && matchesSearch;
      }});
    }}

    function getSelectedNode() {{
      return activeNodeId ? nodeMap.get(activeNodeId) : null;
    }}

    function buildNeighborSet(selectedNode) {{
      if (!selectedNode) {{
        return new Set();
      }}
      const neighbors = new Set([selectedNode.id]);
      for (const link of links) {{
        if (link.source === selectedNode.id) neighbors.add(link.target);
        if (link.target === selectedNode.id) neighbors.add(link.source);
      }}
      return neighbors;
    }}

    function hashValue(text) {{
      return Array.from(text).reduce((acc, ch) => ((acc * 31) + ch.charCodeAt(0)) >>> 0, 7);
    }}

    function sortClusterMembers(members) {{
      return [...members].sort((left, right) => {{
        if (right.metric_fan_in_dependency_graph !== left.metric_fan_in_dependency_graph) {{
          return right.metric_fan_in_dependency_graph - left.metric_fan_in_dependency_graph;
        }}
        if (right.metric_fan_out_dependency_graph !== left.metric_fan_out_dependency_graph) {{
          return right.metric_fan_out_dependency_graph - left.metric_fan_out_dependency_graph;
        }}
        return left.absolute_name.localeCompare(right.absolute_name);
      }});
    }}

    function assignInitialLayout() {{
      const clusterMap = new Map();
      for (const node of nodes) {{
        const members = clusterMap.get(node.community_id) || [];
        members.push(node);
        clusterMap.set(node.community_id, members);
      }}

      const communities = [...clusterMap.keys()].sort((a, b) => a - b);
      const ringCount = Math.max(1, Math.ceil(Math.sqrt(communities.length)));
      const centerX = width / 2;
      const centerY = height / 2;
      const ringRadiusX = 290;
      const ringRadiusY = 220;

      communities.forEach((communityId, communityIndex) => {{
        const members = sortClusterMembers(clusterMap.get(communityId) || []);
        const row = Math.floor(communityIndex / ringCount);
        const col = communityIndex % ringCount;
        const rowOffset = (row - (Math.ceil(communities.length / ringCount) - 1) / 2) * 180;
        const colOffset = (col - (ringCount - 1) / 2) * 220;
        const angle = (communityIndex / Math.max(1, communities.length)) * Math.PI * 2;
        const anchorX = centerX + colOffset + Math.cos(angle) * Math.min(90, ringRadiusX / Math.max(1, communities.length / 8));
        const anchorY = centerY + rowOffset + Math.sin(angle) * Math.min(70, ringRadiusY / Math.max(1, communities.length / 8));

        members.forEach((node, memberIndex) => {{
          const golden = memberIndex * 2.399963229728653;
          const localRadius = 18 + Math.sqrt(memberIndex + 1) * 22 + (hashValue(node.id) % 7);
          node.x = Math.min(width - 30, Math.max(30, anchorX + Math.cos(golden) * localRadius));
          node.y = Math.min(height - 30, Math.max(30, anchorY + Math.sin(golden) * localRadius));
          node.vx = 0;
          node.vy = 0;
        }});
      }});
    }}

    function stabilizeLayout(iterations) {{
      const idealDistance = 84;
      const centerX = width / 2;
      const centerY = height / 2;

      for (let iteration = 0; iteration < iterations; iteration += 1) {{
        const alpha = 1 - (iteration / iterations);
        const clusterAnchors = new Map();
        for (const node of nodes) {{
          const anchor = clusterAnchors.get(node.community_id) || {{ x: 0, y: 0, count: 0 }};
          anchor.x += node.x;
          anchor.y += node.y;
          anchor.count += 1;
          clusterAnchors.set(node.community_id, anchor);
        }}
        for (const anchor of clusterAnchors.values()) {{
          anchor.x /= anchor.count;
          anchor.y /= anchor.count;
        }}

        for (const node of nodes) {{
          let fx = 0;
          let fy = 0;

          for (const other of nodes) {{
            if (node === other) continue;
            const dx = node.x - other.x;
            const dy = node.y - other.y;
            const distanceSq = Math.max(49, dx * dx + dy * dy);
            const distance = Math.sqrt(distanceSq);
            const repulsion = (340 * (0.35 + alpha * 0.9)) / distanceSq;
            fx += (dx / distance) * repulsion;
            fy += (dy / distance) * repulsion;
          }}

          const anchor = clusterAnchors.get(node.community_id);
          fx += (anchor.x - node.x) * (0.015 + alpha * 0.02);
          fy += (anchor.y - node.y) * (0.015 + alpha * 0.02);
          fx += (centerX - node.x) * 0.0012;
          fy += (centerY - node.y) * 0.0012;

          node.vx = (node.vx + fx) * 0.78;
          node.vy = (node.vy + fy) * 0.78;
        }}

        for (const link of links) {{
          const dx = link.targetNode.x - link.sourceNode.x;
          const dy = link.targetNode.y - link.sourceNode.y;
          const distance = Math.max(1, Math.sqrt(dx * dx + dy * dy));
          const spring = (distance - idealDistance) * (0.02 + alpha * 0.016);
          const fx = (dx / distance) * spring;
          const fy = (dy / distance) * spring;
          link.sourceNode.vx += fx;
          link.sourceNode.vy += fy;
          link.targetNode.vx -= fx;
          link.targetNode.vy -= fy;
        }}

        for (const node of nodes) {{
          node.x = Math.min(width - 24, Math.max(24, node.x + node.vx));
          node.y = Math.min(height - 24, Math.max(24, node.y + node.vy));
        }}
      }}

      for (const node of nodes) {{
        node.vx = 0;
        node.vy = 0;
      }}
    }}

    const linkLayer = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    const nodeLayer = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    svg.append(linkLayer, nodeLayer);

    let draggingNode = null;

    function activateGraph(nextGraphType) {{
      graphType = nextGraphType;
      graphPayload = payload.graphs[graphType];
      nodes = graphPayload.graph.nodes.map((node, index) => ({{ ...node, index }}));
      nodeMap = new Map(nodes.map(node => [node.id, node]));
      links = graphPayload.graph.links.map(link => ({{
        ...link,
        sourceNode: nodeMap.get(link.source),
        targetNode: nodeMap.get(link.target)
      }})).filter(link => link.sourceNode && link.targetNode);
      clusters = Object.values(graphPayload.cluster_metrics_map).sort((a, b) => a.community_id - b.community_id);
      stats = graphPayload.statistics;
      activeCluster = null;
      activeNodeId = null;
      draggingNode = null;
      selectionEl.className = 'empty';
      selectionEl.textContent = 'Hover or click a node to inspect node metrics.';
      assignInitialLayout();
      stabilizeLayout(160);
      renderStats();
      renderClusters();
      renderFiles();
      draw();
    }}

    function draw() {{
      const visibleIds = new Set(getVisibleNodes().map(node => node.id));
      const visibleLinks = links.filter(link => visibleIds.has(link.source) && visibleIds.has(link.target));
      const selectedNode = getSelectedNode();
      const neighborSet = buildNeighborSet(selectedNode);
      linkLayer.innerHTML = visibleLinks.map(link => {{
        const isActive = !!(selectedNode && (link.source === selectedNode.id || link.target === selectedNode.id));
        const isSameCluster = link.sourceNode.community_id === link.targetNode.community_id;
        const strokeColor = isActive ? 'var(--accent-2)' : isSameCluster ? colorForCommunity(link.sourceNode.community_id) : 'var(--edge)';
        const strokeW = isActive ? 3.2 : 1.2;
        const strokeO = (selectedNode && !isActive) ? 0.08 : isSameCluster ? 0.45 : 0.85;
        return `
          <line
            x1="${{link.sourceNode.x.toFixed(1)}}"
            y1="${{link.sourceNode.y.toFixed(1)}}"
            x2="${{link.targetNode.x.toFixed(1)}}"
            y2="${{link.targetNode.y.toFixed(1)}}"
            stroke="${{strokeColor}}"
            stroke-width="${{strokeW}}"
            stroke-opacity="${{strokeO}}"
          />
        `;
      }}).join('');
      nodeLayer.innerHTML = getVisibleNodes().map(node => {{
        const radius = 6 + Math.sqrt(node.metric_fan_in_dependency_graph + node.metric_fan_out_dependency_graph + 1) * 2;
        const highlighted = activeNodeId === node.id;
        const related = !selectedNode || neighborSet.has(node.id);
        const fill = colorForCommunity(node.community_id);
        const showLabel = highlighted || (selectedNode && related) || (!selectedNode && (node.metric_fan_in_dependency_graph + node.metric_fan_out_dependency_graph >= 3));
        return `
          <g data-node-id="${{node.id}}" transform="translate(${{node.x.toFixed(1)}} ${{node.y.toFixed(1)}})">
            <circle r="${{radius.toFixed(1)}}" fill="${{fill}}" fill-opacity="${{related ? 0.96 : 0.16}}" stroke="${{highlighted ? 'var(--accent-2)' : 'rgba(255,255,255,0.85)'}}" stroke-width="${{highlighted ? 3 : 1.5}}" stroke-opacity="${{related ? 1 : 0.3}}"></circle>
            <text y="-${{radius + 8}}" text-anchor="middle" font-size="11" fill="var(--ink)" fill-opacity="${{showLabel ? (related ? 1 : 0.26) : 0}}">${{node.display_name}}</text>
          </g>
        `;
      }}).join('');

      nodeLayer.querySelectorAll('[data-node-id]').forEach(group => {{
        group.addEventListener('mouseenter', () => {{
          const node = nodeMap.get(group.getAttribute('data-node-id'));
          updateSelection(node);
        }});
        group.addEventListener('mouseleave', () => {{
          updateSelection(activeNodeId ? nodeMap.get(activeNodeId) : null);
        }});
        group.addEventListener('mousedown', event => {{
          const node = nodeMap.get(group.getAttribute('data-node-id'));
          draggingNode = node;
          event.preventDefault();
        }});
        group.addEventListener('click', () => {{
          const node = nodeMap.get(group.getAttribute('data-node-id'));
          activeNodeId = activeNodeId === node.id ? null : node.id;
          updateSelection(activeNodeId ? nodeMap.get(activeNodeId) : null);
          renderFiles();
          draw();
        }});
      }});
    }}

    svg.addEventListener('mousemove', event => {{
      if (!draggingNode) return;
      const rect = svg.getBoundingClientRect();
      const x = ((event.clientX - rect.left) / rect.width) * width;
      const y = ((event.clientY - rect.top) / rect.height) * height;
      draggingNode.x = x;
      draggingNode.y = y;
      draggingNode.vx = 0;
      draggingNode.vy = 0;
      draw();
    }});

    svg.addEventListener('click', event => {{
      if (event.target === svg) {{
        activeNodeId = null;
        updateSelection(null);
        renderFiles();
        draw();
      }}
    }});

    window.addEventListener('mouseup', () => {{
      draggingNode = null;
    }});

    searchEl.addEventListener('input', () => {{
      searchTerm = searchEl.value.trim().toLowerCase();
      renderFiles();
      draw();
    }});

    graphTypeEl.innerHTML = payload.analysis_config.graph_types.map(type => `
      <option value="${{type}}"${{type === payload.analysis_config.default_graph_type ? ' selected' : ''}}>${{graphTypeLabels[type] || type}}</option>
    `).join('');
    graphTypeEl.addEventListener('change', () => {{
      activateGraph(graphTypeEl.value);
    }});

    activateGraph(graphType);
  </script>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::{build_export, render_emerge_json};
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
                    ws_complexity: 4,
                    tfidf: 0.6,
                    incoming_dependency_count: 1,
                    outgoing_dependency_count: 0,
                    incoming_inheritance_count: 0,
                    outgoing_inheritance_count: 1,
                    louvain_modularity: 0,
                },
            ],
        };

        let export = build_export(&graph, &metrics);
        let json = render_emerge_json(&graph, &metrics).expect("render export");

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
    }
}
