use crate::error::Result;
use crate::fs;
use crate::model::{DependencyKind, GraphReport, MetricsReport, TOOL_NAME, TOOL_VERSION};
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
    metric_file_result_dependency_graph_louvain_modularity: f64,
    metric_file_result_dependency_graph_community_count: usize,
    metric_file_result_dependency_graph_total_sloc: usize,
    metric_file_result_dependency_graph_total_parser_warnings: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ClusterMetrics {
    community_id: usize,
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
    let community_assignment = detect_communities(graph);
    let community_count = community_assignment
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .len();

    let nodes = graph
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
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
                community_id: community_assignment[index],
                metric_sloc_in_file: file.line_count,
                metric_fan_in_dependency_graph: metric.incoming_dependency_count,
                metric_fan_out_dependency_graph: metric.outgoing_dependency_count,
                metric_file_result_dependency_graph_louvain_modularity_in_file:
                    community_assignment[index],
                metric_file_hotspot_score: metric.file_hotspot_score,
            }
        })
        .collect::<Vec<_>>();

    let links = graph
        .edges
        .iter()
        .map(|edge| BrowserLink {
            source: edge.from.clone(),
            target: edge.to.clone(),
            kind: dependency_kind_label(&edge.kind).to_string(),
            line: edge.line,
        })
        .collect::<Vec<_>>();

    let total_sloc = nodes.iter().map(|node| node.line_count).sum::<usize>();
    let cluster_metrics_map = build_cluster_metrics(&nodes, total_sloc);
    let group_count = graph
        .files
        .iter()
        .map(|file| file.group.as_deref().unwrap_or("ungrouped"))
        .collect::<BTreeSet<_>>()
        .len();
    let modularity = compute_modularity(graph, &community_assignment);

    BrowserExport {
        metadata: BrowserMetadata {
            tool_name: TOOL_NAME,
            tool_version: TOOL_VERSION,
            export_format: "emerge-compatible-browser",
            project_name,
            root: graph.root.clone(),
        },
        analysis_config: AnalysisConfig {
            graph_types: vec!["file_result_dependency_graph"],
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
                    edge_count: graph.edges.len(),
                    cycle_count: graph.cycles.len(),
                    parser_warning_count: graph.parser_warnings.len(),
                    community_count,
                    group_count,
                    total_sloc,
                },
                overall_metric_results: OverallMetricResults {
                    metric_file_result_dependency_graph_louvain_modularity: modularity,
                    metric_file_result_dependency_graph_community_count: community_count,
                    metric_file_result_dependency_graph_total_sloc: total_sloc,
                    metric_file_result_dependency_graph_total_parser_warnings: graph
                        .parser_warnings
                        .len(),
                },
                cluster_metrics_map,
            },
        },
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

    accumulators
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
        .collect()
}

fn average(total: usize, count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn detect_communities(graph: &GraphReport) -> Vec<usize> {
    let node_count = graph.files.len();
    if node_count == 0 {
        return Vec::new();
    }

    let index_by_file = graph
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let adjacency = build_undirected_adjacency(graph, &index_by_file);
    let mut communities = (0..node_count).collect::<Vec<_>>();

    if graph.edges.is_empty() {
        return communities;
    }

    let mut improved = true;
    while improved {
        improved = false;
        for node_index in 0..node_count {
            let current = communities[node_index];
            let baseline = compute_modularity_from_adjacency(&adjacency, &communities);
            let mut best_score = baseline;
            let mut best_community = current;
            let mut candidates = adjacency[node_index]
                .iter()
                .map(|(neighbor, _)| communities[*neighbor])
                .collect::<BTreeSet<_>>();
            candidates.insert(current);

            for candidate in candidates {
                if candidate == current {
                    continue;
                }

                communities[node_index] = candidate;
                let score = compute_modularity_from_adjacency(&adjacency, &communities);
                if score > best_score + 1e-9
                    || ((score - best_score).abs() <= 1e-9 && candidate < best_community)
                {
                    best_score = score;
                    best_community = candidate;
                }
            }

            communities[node_index] = best_community;
            if best_community != current {
                improved = true;
            }
        }
    }

    renumber_communities(&graph.files, &communities)
}

fn renumber_communities(files: &[crate::model::ProjectFile], communities: &[usize]) -> Vec<usize> {
    let mut files_by_community = BTreeMap::<usize, Vec<&str>>::new();
    for (index, file) in files.iter().enumerate() {
        files_by_community
            .entry(communities[index])
            .or_default()
            .push(file.path.as_str());
    }

    let ordered = files_by_community
        .into_iter()
        .map(|(community, mut files)| {
            files.sort();
            (community, files[0].to_string())
        })
        .collect::<Vec<_>>();
    let mut ordered = ordered;
    ordered.sort_by(|left, right| left.1.cmp(&right.1));

    let remap = ordered
        .into_iter()
        .enumerate()
        .map(|(new_id, (old_id, _))| (old_id, new_id))
        .collect::<BTreeMap<_, _>>();

    communities
        .iter()
        .map(|community| remap[community])
        .collect::<Vec<_>>()
}

fn build_undirected_adjacency(
    graph: &GraphReport,
    index_by_file: &BTreeMap<&str, usize>,
) -> Vec<Vec<(usize, f64)>> {
    let mut edge_weights = BTreeMap::<(usize, usize), f64>::new();

    for edge in &graph.edges {
        let Some(&source) = index_by_file.get(edge.from.as_str()) else {
            continue;
        };
        let Some(&target) = index_by_file.get(edge.to.as_str()) else {
            continue;
        };
        if source == target {
            continue;
        }

        let pair = if source < target {
            (source, target)
        } else {
            (target, source)
        };
        *edge_weights.entry(pair).or_insert(0.0) += 1.0;
    }

    let mut adjacency = vec![Vec::<(usize, f64)>::new(); graph.files.len()];
    for ((source, target), weight) in edge_weights {
        adjacency[source].push((target, weight));
        adjacency[target].push((source, weight));
    }
    for neighbors in &mut adjacency {
        neighbors.sort_by(|left, right| left.0.cmp(&right.0));
    }
    adjacency
}

fn compute_modularity(graph: &GraphReport, communities: &[usize]) -> f64 {
    let index_by_file = graph
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let adjacency = build_undirected_adjacency(graph, &index_by_file);
    compute_modularity_from_adjacency(&adjacency, communities)
}

fn compute_modularity_from_adjacency(
    adjacency: &[Vec<(usize, f64)>],
    communities: &[usize],
) -> f64 {
    let mut total_weight = 0.0;
    let mut internal_weight_by_community = BTreeMap::<usize, f64>::new();
    let mut degree_by_community = BTreeMap::<usize, f64>::new();

    for (node_index, neighbors) in adjacency.iter().enumerate() {
        let community = communities[node_index];
        let degree = neighbors.iter().map(|(_, weight)| *weight).sum::<f64>();
        *degree_by_community.entry(community).or_insert(0.0) += degree;

        for (neighbor, weight) in neighbors {
            if node_index < *neighbor {
                total_weight += *weight;
                if communities[*neighbor] == community {
                    *internal_weight_by_community.entry(community).or_insert(0.0) += *weight;
                }
            }
        }
    }

    if total_weight <= f64::EPSILON {
        return 0.0;
    }

    degree_by_community
        .into_iter()
        .map(|(community, degree_sum)| {
            let internal = internal_weight_by_community
                .get(&community)
                .copied()
                .unwrap_or(0.0);
            (internal / total_weight) - (degree_sum / (2.0 * total_weight)).powi(2)
        })
        .sum()
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
          <h2>Visible Files</h2>
          <div id="files" class="scroll"></div>
        </section>
      </div>
    </aside>
  </div>
  <script id="graph-data" type="application/json">{data_json}</script>
  <script>
    const payload = JSON.parse(document.getElementById('graph-data').textContent);
    const graphType = payload.analysis_config.default_graph_type;
    const graphPayload = payload.graphs[graphType];
    const nodes = graphPayload.graph.nodes.map((node, index) => ({{ ...node, index }}));
    const nodeMap = new Map(nodes.map(node => [node.id, node]));
    const links = graphPayload.graph.links.map(link => ({{
      ...link,
      sourceNode: nodeMap.get(link.source),
      targetNode: nodeMap.get(link.target)
    }})).filter(link => link.sourceNode && link.targetNode);
    const clusters = Object.values(graphPayload.cluster_metrics_map).sort((a, b) => a.community_id - b.community_id);
    const stats = graphPayload.statistics;
    const selectionEl = document.getElementById('selection');
    const clustersEl = document.getElementById('clusters');
    const filesEl = document.getElementById('files');
    const searchEl = document.getElementById('search');
    const svg = document.getElementById('graph');
    const statsEl = document.getElementById('stats');
    const width = 960;
    const height = 720;
    const MAX_SIMULATION_TICKS = 220;
    const STABLE_ALPHA_THRESHOLD = 0.012;
    let activeCluster = null;
    let searchTerm = '';
    let activeNodeId = null;
    let simulationTick = 0;
    let simulationRunning = true;

    function colorForCommunity(communityId) {{
      const hue = (communityId * 57) % 360;
      return `hsl(${{hue}} 62% 46%)`;
    }}

    function formatNumber(value) {{
      return Number.isInteger(value) ? String(value) : value.toFixed(2);
    }}

    function renderStats() {{
      const cards = [
        ['Files', stats.node_count],
        ['Edges', stats.edge_count],
        ['Clusters', stats.community_count],
        ['SLOC', stats.total_sloc],
      ];
      statsEl.innerHTML = cards.map(([label, value]) => `
        <div class="stat">
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
            Cluster ${{cluster.community_id}}
          </div>
          <div class="meta">${{cluster.node_count}} files, ${{cluster.total_sloc}} SLOC, fan-in ${{formatNumber(cluster.avg_fan_in)}} / fan-out ${{formatNumber(cluster.avg_fan_out)}}</div>
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
          <h3>Cluster ${{cluster.community_id}}</h3>
          <p class="meta">${{cluster.node_count}} files, ${{cluster.total_sloc}} SLOC, ${{cluster.percent_total_sloc.toFixed(1)}}% of project SLOC</p>
          <div class="detail-grid">
            <div class="detail-card"><span class="meta">Avg fan-in</span><strong>${{formatNumber(cluster.avg_fan_in)}}</strong></div>
            <div class="detail-card"><span class="meta">Avg fan-out</span><strong>${{formatNumber(cluster.avg_fan_out)}}</strong></div>
            <div class="detail-card"><span class="meta">Avg functions</span><strong>${{formatNumber(cluster.avg_function_count)}}</strong></div>
            <div class="detail-card"><span class="meta">Files</span><strong>${{cluster.node_count}}</strong></div>
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
          <div class="detail-card"><span class="meta">Lines</span><strong>${{node.line_count}}</strong></div>
          <div class="detail-card"><span class="meta">Fan-in</span><strong>${{node.metric_fan_in_dependency_graph}}</strong></div>
          <div class="detail-card"><span class="meta">Fan-out</span><strong>${{node.metric_fan_out_dependency_graph}}</strong></div>
          <div class="detail-card"><span class="meta">Community</span><strong>${{node.community_id}}</strong></div>
          <div class="detail-card"><span class="meta">Functions</span><strong>${{node.function_count}}</strong></div>
          <div class="detail-card"><span class="meta">Hotspot</span><strong>${{node.metric_file_hotspot_score}}</strong></div>
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

    function seededPosition(node) {{
      const hash = Array.from(node.id).reduce((acc, ch) => ((acc * 31) + ch.charCodeAt(0)) >>> 0, 7);
      const clusterAngle = (node.community_id * 1.61803398875) % (Math.PI * 2);
      const radius = 120 + (hash % 160);
      return {{
        x: width / 2 + Math.cos(clusterAngle + (hash % 360) * Math.PI / 180) * radius,
        y: height / 2 + Math.sin(clusterAngle + (hash % 360) * Math.PI / 180) * radius
      }};
    }}

    nodes.forEach(node => {{
      const pos = seededPosition(node);
      node.x = pos.x;
      node.y = pos.y;
      node.vx = 0;
      node.vy = 0;
    }});

    const linkLayer = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    const nodeLayer = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    svg.append(linkLayer, nodeLayer);

    let draggingNode = null;

    function draw() {{
      const visibleIds = new Set(getVisibleNodes().map(node => node.id));
      const visibleLinks = links.filter(link => visibleIds.has(link.source) && visibleIds.has(link.target));
      linkLayer.innerHTML = visibleLinks.map(link => `
        <line
          x1="${{link.sourceNode.x.toFixed(1)}}"
          y1="${{link.sourceNode.y.toFixed(1)}}"
          x2="${{link.targetNode.x.toFixed(1)}}"
          y2="${{link.targetNode.y.toFixed(1)}}"
          stroke="var(--edge)"
          stroke-width="1.2"
        />
      `).join('');
      nodeLayer.innerHTML = getVisibleNodes().map(node => {{
        const radius = 6 + Math.sqrt(node.metric_fan_in_dependency_graph + node.metric_fan_out_dependency_graph + 1) * 2;
        const highlighted = activeNodeId === node.id;
        return `
          <g data-node-id="${{node.id}}" transform="translate(${{node.x.toFixed(1)}} ${{node.y.toFixed(1)}})">
            <circle r="${{radius.toFixed(1)}}" fill="${{colorForCommunity(node.community_id)}}" stroke="${{highlighted ? 'var(--accent-2)' : 'rgba(255,255,255,0.85)'}}" stroke-width="${{highlighted ? 3 : 1.5}}"></circle>
            <text y="-${{radius + 8}}" text-anchor="middle" font-size="11" fill="var(--ink)">${{node.display_name}}</text>
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
          activeNodeId = node.id;
          updateSelection(node);
          renderFiles();
          draw();
          event.preventDefault();
        }});
        group.addEventListener('click', () => {{
          const node = nodeMap.get(group.getAttribute('data-node-id'));
          activeNodeId = node.id;
          updateSelection(node);
          renderFiles();
          draw();
        }});
      }});
    }}

    function simulate() {{
      if (!simulationRunning) {{
        return;
      }}

      const visible = getVisibleNodes();
      const visibleSet = new Set(visible.map(node => node.id));
      const visibleLinks = links.filter(link => visibleSet.has(link.source) && visibleSet.has(link.target));
      let totalVelocity = 0;

      for (const node of visible) {{
        let ax = 0;
        let ay = 0;
        for (const other of visible) {{
          if (node === other) continue;
          const dx = node.x - other.x;
          const dy = node.y - other.y;
          const distanceSq = Math.max(36, dx * dx + dy * dy);
          const repulsion = 1400 / distanceSq;
          ax += (dx / Math.sqrt(distanceSq)) * repulsion;
          ay += (dy / Math.sqrt(distanceSq)) * repulsion;
        }}

        const clusterAngle = (node.community_id * 57) * Math.PI / 180;
        const targetX = width / 2 + Math.cos(clusterAngle) * 160;
        const targetY = height / 2 + Math.sin(clusterAngle) * 160;
        ax += (targetX - node.x) * 0.0025;
        ay += (targetY - node.y) * 0.0025;
        ax += (width / 2 - node.x) * 0.0008;
        ay += (height / 2 - node.y) * 0.0008;

        node.vx = (node.vx + ax) * 0.88;
        node.vy = (node.vy + ay) * 0.88;
      }}

      for (const link of visibleLinks) {{
        const dx = link.targetNode.x - link.sourceNode.x;
        const dy = link.targetNode.y - link.sourceNode.y;
        const distance = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        const force = (distance - 86) * 0.0022;
        const fx = (dx / distance) * force;
        const fy = (dy / distance) * force;
        link.sourceNode.vx += fx;
        link.sourceNode.vy += fy;
        link.targetNode.vx -= fx;
        link.targetNode.vy -= fy;
      }}

      for (const node of visible) {{
        if (draggingNode && draggingNode.id === node.id) continue;
        node.x = Math.min(width - 24, Math.max(24, node.x + node.vx));
        node.y = Math.min(height - 24, Math.max(24, node.y + node.vy));
        totalVelocity += Math.abs(node.vx) + Math.abs(node.vy);
      }}

      draw();
      simulationTick += 1;
      const averageVelocity = visible.length === 0 ? 0 : totalVelocity / visible.length;
      if (simulationTick >= MAX_SIMULATION_TICKS || averageVelocity <= STABLE_ALPHA_THRESHOLD) {{
        simulationRunning = false;
        for (const node of nodes) {{
          node.vx = 0;
          node.vy = 0;
        }}
        draw();
        return;
      }}

      requestAnimationFrame(simulate);
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

    window.addEventListener('mouseup', () => {{
      draggingNode = null;
    }});

    searchEl.addEventListener('input', () => {{
      searchTerm = searchEl.value.trim().toLowerCase();
      renderFiles();
      draw();
    }});

    renderStats();
    renderClusters();
    renderFiles();
    draw();
    simulate();
  </script>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::{build_export, render_emerge_json};
    use crate::model::{
        DependencyEdge, DependencyKind, FileMetric, GraphReport, MetricsReport, OutputMetadata,
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
        };
        let metrics = MetricsReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            files: vec![
                FileMetric {
                    file: "a.gd".to_string(),
                    line_count: 20,
                    function_count: 2,
                    signal_count: 0,
                    public_function_count: 1,
                    outgoing_dependency_count: 1,
                    incoming_dependency_count: 1,
                    cycle_count: 1,
                    file_hotspot_score: 14,
                },
                FileMetric {
                    file: "b.gd".to_string(),
                    line_count: 15,
                    function_count: 1,
                    signal_count: 1,
                    public_function_count: 1,
                    outgoing_dependency_count: 1,
                    incoming_dependency_count: 1,
                    cycle_count: 1,
                    file_hotspot_score: 12,
                },
                FileMetric {
                    file: "c.gd".to_string(),
                    line_count: 10,
                    function_count: 1,
                    signal_count: 0,
                    public_function_count: 1,
                    outgoing_dependency_count: 0,
                    incoming_dependency_count: 0,
                    cycle_count: 0,
                    file_hotspot_score: 5,
                },
            ],
            functions: Vec::new(),
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
