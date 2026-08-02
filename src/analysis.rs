use crate::error::{GdHealthError, Result};
use crate::metrics::analyze_file;
use crate::model::{
    DependencyEdge, DependencyKind, EntityEdge, EntityMetric, EntityNode, EntityScanReport,
    FileMetric, GraphAggregateMetrics, GraphCommunity, GraphReport,
};
use crate::parser::extract::parse_file;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub struct FileGraphAnalysis {
    pub communities: Vec<GraphCommunity>,
    pub metrics: GraphAggregateMetrics,
}

pub fn analyze_file_graph(graph: &GraphReport) -> FileGraphAnalysis {
    let node_ids = graph
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let edges = graph
        .edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect::<Vec<_>>();
    let communities = detect_communities(&node_ids, &edges);
    let community_count = communities
        .iter()
        .map(|item| item.community_id)
        .collect::<BTreeSet<_>>()
        .len();
    let group_count = graph
        .files
        .iter()
        .map(|file| file.group.as_deref().unwrap_or("ungrouped"))
        .collect::<BTreeSet<_>>()
        .len();
    let total_sloc = graph
        .files
        .iter()
        .map(|file| file.line_count)
        .sum::<usize>();
    let modularity = compute_modularity_for_ids(&node_ids, &edges, &communities);

    FileGraphAnalysis {
        communities,
        metrics: GraphAggregateMetrics {
            louvain_modularity: modularity,
            community_count,
            total_sloc,
            group_count,
        },
    }
}

pub fn build_entity_scan(
    root: &Path,
    graph: &GraphReport,
    file_metrics: &[FileMetric],
) -> Result<EntityScanReport> {
    let file_metric_map = file_metrics
        .iter()
        .map(|metric| (metric.file.as_str(), metric))
        .collect::<BTreeMap<_, _>>();
    let mut entities = Vec::new();
    let mut entity_by_file = BTreeMap::<String, String>::new();

    for file in &graph.files {
        if !file.path.ends_with(".gd") {
            continue;
        }

        let absolute_path = root.join(&file.path);
        let content =
            fs::read_to_string(&absolute_path).map_err(|source| GdHealthError::ReadFile {
                path: absolute_path.clone(),
                source,
            })?;
        let parsed = parse_file(&file.path, &content);
        let analysis = analyze_file(&file.path, &content);
        let class_name = parsed.class_name.as_ref().map(|item| item.name.clone());
        let entity_id = class_name.clone().unwrap_or_else(|| file.path.clone());
        let display_name = class_name
            .clone()
            .or_else(|| {
                Path::new(&file.path)
                    .file_stem()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| file.path.clone());

        entity_by_file.insert(file.path.clone(), entity_id.clone());
        entities.push(EntityNode {
            id: entity_id,
            display_name,
            file: file.path.clone(),
            class_name,
            extends: None,
            start_line: 1,
            end_line: file.line_count.max(1),
            line_count: file.line_count,
            method_count: analysis.functions.len(),
        });
    }

    let mut dependency_edges = Vec::new();
    let mut inheritance_edges = Vec::new();
    for edge in &graph.edges {
        let Some(from_entity) = entity_by_file.get(&edge.from) else {
            continue;
        };
        let Some(to_entity) = entity_by_file.get(&edge.to) else {
            continue;
        };

        let entity_edge = EntityEdge {
            from: from_entity.clone(),
            to: to_entity.clone(),
            kind: edge.kind.clone(),
            line: edge.line,
        };

        if matches!(edge.kind, DependencyKind::Extends) {
            inheritance_edges.push(entity_edge);
        } else {
            dependency_edges.push(entity_edge);
        }
    }

    dependency_edges.sort_by(edge_sort_key);
    dependency_edges.dedup_by(edge_dedup_key);
    inheritance_edges.sort_by(edge_sort_key);
    inheritance_edges.dedup_by(edge_dedup_key);

    let mut complete_edges = dependency_edges.clone();
    complete_edges.extend(inheritance_edges.clone());
    complete_edges.sort_by(edge_sort_key);
    complete_edges.dedup_by(edge_dedup_key);

    let entity_ids = entities
        .iter()
        .map(|entity| entity.id.clone())
        .collect::<Vec<_>>();
    let complete_pairs = complete_edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect::<Vec<_>>();
    let communities = detect_communities(&entity_ids, &complete_pairs);
    let community_count = communities
        .iter()
        .map(|item| item.community_id)
        .collect::<BTreeSet<_>>()
        .len();
    let total_sloc = entities
        .iter()
        .map(|entity| entity.line_count)
        .sum::<usize>();
    let modularity = compute_modularity_for_ids(&entity_ids, &complete_pairs, &communities);

    let extends_by_entity = inheritance_edges
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.clone()))
        .collect::<BTreeMap<_, _>>();
    for entity in &mut entities {
        entity.extends = extends_by_entity.get(entity.id.as_str()).cloned();
    }

    let _ = file_metric_map;

    Ok(EntityScanReport {
        entities,
        dependency_edges,
        inheritance_edges,
        complete_edges,
        communities,
        complete_graph_metrics: GraphAggregateMetrics {
            louvain_modularity: modularity,
            community_count,
            total_sloc,
            group_count: 0,
        },
    })
}

pub fn build_entity_metrics(
    entity_scan: &EntityScanReport,
    file_metrics: &[FileMetric],
) -> Vec<EntityMetric> {
    let file_metric_map = file_metrics
        .iter()
        .map(|metric| (metric.file.as_str(), metric))
        .collect::<BTreeMap<_, _>>();
    let community_map = entity_scan
        .communities
        .iter()
        .map(|item| (item.id.as_str(), item.community_id))
        .collect::<BTreeMap<_, _>>();
    let mut dependency_incoming = BTreeMap::<&str, usize>::new();
    let mut dependency_outgoing = BTreeMap::<&str, usize>::new();
    let mut inheritance_incoming = BTreeMap::<&str, usize>::new();
    let mut inheritance_outgoing = BTreeMap::<&str, usize>::new();

    for edge in &entity_scan.dependency_edges {
        *dependency_outgoing.entry(edge.from.as_str()).or_insert(0) += 1;
        *dependency_incoming.entry(edge.to.as_str()).or_insert(0) += 1;
    }
    for edge in &entity_scan.inheritance_edges {
        *inheritance_outgoing.entry(edge.from.as_str()).or_insert(0) += 1;
        *inheritance_incoming.entry(edge.to.as_str()).or_insert(0) += 1;
    }

    let mut metrics = entity_scan
        .entities
        .iter()
        .map(|entity| {
            let file_metric = file_metric_map
                .get(entity.file.as_str())
                .expect("entity should map to file metrics");
            EntityMetric {
                entity: entity.id.clone(),
                file: entity.file.clone(),
                line_count: entity.line_count,
                method_count: entity.method_count,
                signal_count: file_metric.signal_count,
                signal_emit_count: file_metric.signal_emit_count,
                signal_connection_count: file_metric.signal_connection_count,
                unresolved_signal_reference_count: file_metric.unresolved_signal_reference_count,
                ws_complexity: file_metric.ws_complexity,
                tfidf: file_metric.tfidf,
                incoming_dependency_count: dependency_incoming
                    .get(entity.id.as_str())
                    .copied()
                    .unwrap_or(0),
                outgoing_dependency_count: dependency_outgoing
                    .get(entity.id.as_str())
                    .copied()
                    .unwrap_or(0),
                incoming_inheritance_count: inheritance_incoming
                    .get(entity.id.as_str())
                    .copied()
                    .unwrap_or(0),
                outgoing_inheritance_count: inheritance_outgoing
                    .get(entity.id.as_str())
                    .copied()
                    .unwrap_or(0),
                louvain_modularity: community_map.get(entity.id.as_str()).copied().unwrap_or(0),
            }
        })
        .collect::<Vec<_>>();

    metrics.sort_by(|left, right| left.entity.cmp(&right.entity));
    metrics
}

pub fn compute_tfidf_scores(root: &Path, graph: &GraphReport) -> Result<BTreeMap<String, f64>> {
    let mut docs = Vec::<(String, Vec<String>)>::new();
    let mut doc_frequency = BTreeMap::<String, usize>::new();

    for file in &graph.files {
        let absolute_path = root.join(&file.path);
        let content =
            fs::read_to_string(&absolute_path).map_err(|source| GdHealthError::ReadFile {
                path: absolute_path.clone(),
                source,
            })?;
        let tokens = tokenize(&content);
        let unique = tokens.iter().cloned().collect::<BTreeSet<_>>();
        for token in unique {
            *doc_frequency.entry(token).or_insert(0) += 1;
        }
        docs.push((file.path.clone(), tokens));
    }

    let doc_count = docs.len() as f64;
    let mut scores = BTreeMap::new();
    for (path, tokens) in docs {
        if tokens.is_empty() {
            scores.insert(path, 0.0);
            continue;
        }

        let mut term_frequency = BTreeMap::<String, usize>::new();
        for token in tokens {
            *term_frequency.entry(token).or_insert(0) += 1;
        }

        let total_terms = term_frequency.values().sum::<usize>() as f64;
        let score = term_frequency
            .into_iter()
            .map(|(token, count)| {
                let tf = count as f64 / total_terms;
                let df = doc_frequency.get(&token).copied().unwrap_or(1) as f64;
                tf * ((doc_count / df).ln() + 1.0)
            })
            .fold(0.0, f64::max);
        scores.insert(path, score);
    }

    Ok(scores)
}

fn tokenize(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in content.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens.into_iter().filter(|token| token.len() > 2).collect()
}

fn edge_sort_key(left: &EntityEdge, right: &EntityEdge) -> std::cmp::Ordering {
    left.from
        .cmp(&right.from)
        .then_with(|| left.to.cmp(&right.to))
        .then_with(|| left.line.cmp(&right.line))
}

fn edge_dedup_key(left: &mut EntityEdge, right: &mut EntityEdge) -> bool {
    left.from == right.from && left.to == right.to && left.line == right.line
}

pub fn detect_communities(
    node_ids: &[String],
    directed_edges: &[(String, String)],
) -> Vec<GraphCommunity> {
    if node_ids.is_empty() {
        return Vec::new();
    }

    let index_by_id = node_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let adjacency = build_undirected_adjacency(node_ids.len(), directed_edges, &index_by_id);
    let mut communities = (0..node_ids.len()).collect::<Vec<_>>();

    if directed_edges.is_empty() {
        return node_ids
            .iter()
            .enumerate()
            .map(|(index, id)| GraphCommunity {
                id: id.clone(),
                community_id: index,
            })
            .collect();
    }

    let mut improved = true;
    while improved {
        improved = false;
        for node_index in 0..node_ids.len() {
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

    let renumbered = renumber_communities(node_ids, &communities);
    node_ids
        .iter()
        .enumerate()
        .map(|(index, id)| GraphCommunity {
            id: id.clone(),
            community_id: renumbered[index],
        })
        .collect()
}

fn renumber_communities(node_ids: &[String], communities: &[usize]) -> Vec<usize> {
    let mut ids_by_community = BTreeMap::<usize, Vec<&str>>::new();
    for (index, id) in node_ids.iter().enumerate() {
        ids_by_community
            .entry(communities[index])
            .or_default()
            .push(id.as_str());
    }

    let mut ordered = ids_by_community
        .into_iter()
        .map(|(community, mut ids)| {
            ids.sort();
            (community, ids[0].to_string())
        })
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.1.cmp(&right.1));

    let remap = ordered
        .into_iter()
        .enumerate()
        .map(|(new_id, (old_id, _))| (old_id, new_id))
        .collect::<BTreeMap<_, _>>();
    communities
        .iter()
        .map(|community| remap[community])
        .collect()
}

fn build_undirected_adjacency(
    node_count: usize,
    directed_edges: &[(String, String)],
    index_by_id: &BTreeMap<&str, usize>,
) -> Vec<Vec<(usize, f64)>> {
    let mut edge_weights = BTreeMap::<(usize, usize), f64>::new();

    for (source_id, target_id) in directed_edges {
        let Some(&source) = index_by_id.get(source_id.as_str()) else {
            continue;
        };
        let Some(&target) = index_by_id.get(target_id.as_str()) else {
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

    let mut adjacency = vec![Vec::<(usize, f64)>::new(); node_count];
    for ((source, target), weight) in edge_weights {
        adjacency[source].push((target, weight));
        adjacency[target].push((source, weight));
    }
    for neighbors in &mut adjacency {
        neighbors.sort_by(|left, right| left.0.cmp(&right.0));
    }
    adjacency
}

pub fn compute_modularity_for_ids(
    node_ids: &[String],
    directed_edges: &[(String, String)],
    communities: &[GraphCommunity],
) -> f64 {
    let index_by_id = node_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let adjacency = build_undirected_adjacency(node_ids.len(), directed_edges, &index_by_id);
    let ordered = node_ids
        .iter()
        .map(|id| {
            communities
                .iter()
                .find(|item| item.id == *id)
                .map(|item| item.community_id)
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    compute_modularity_from_adjacency(&adjacency, &ordered)
}

fn compute_modularity_from_adjacency(
    adjacency: &[Vec<(usize, f64)>],
    communities: &[usize],
) -> f64 {
    let mut total_weight = 0.0;
    let mut internal_weight_by_community = BTreeMap::<usize, f64>::new();
    let mut degree_weight_by_community = BTreeMap::<usize, f64>::new();

    for (node_index, neighbors) in adjacency.iter().enumerate() {
        let degree_weight = neighbors.iter().map(|(_, weight)| *weight).sum::<f64>();
        total_weight += degree_weight;
        *degree_weight_by_community
            .entry(communities[node_index])
            .or_insert(0.0) += degree_weight;

        for (neighbor, weight) in neighbors {
            if node_index < *neighbor && communities[node_index] == communities[*neighbor] {
                *internal_weight_by_community
                    .entry(communities[node_index])
                    .or_insert(0.0) += *weight;
            }
        }
    }

    let m = total_weight / 2.0;
    if m <= f64::EPSILON {
        return 0.0;
    }

    degree_weight_by_community
        .into_iter()
        .map(|(community, degree_weight)| {
            let internal_weight = internal_weight_by_community
                .get(&community)
                .copied()
                .unwrap_or(0.0);
            (internal_weight / m) - (degree_weight / (2.0 * m)).powi(2)
        })
        .sum()
}

pub fn dependency_pairs(edges: &[DependencyEdge]) -> Vec<(String, String)> {
    edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect()
}
