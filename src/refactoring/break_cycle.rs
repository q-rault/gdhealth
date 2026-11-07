//! Break Cycle: for each dependency cycle gdhealth's Tarjan SCC detection
//! already found, name the specific edge(s) to cut. Graph-only — reuses
//! `graph::find_cycles` on trial subgraphs, no new parsing needed.

use crate::config::RefactoringConfig;
use crate::graph::find_cycles;
use crate::model::{DependencyEdge, GraphReport, PolicyFinding, RefactorSuggestion};
use std::collections::BTreeSet;

pub fn annotate(findings: &mut [PolicyFinding], graph: &GraphReport, config: &RefactoringConfig) {
    for finding in findings.iter_mut() {
        if finding.rule_id != "cycle_detected" {
            continue;
        }
        let cycle = &finding.related_files;
        if cycle.is_empty() || cycle.len() > config.max_cycle_files {
            continue;
        }
        if let Some(cut_edges) = find_cut_edges(cycle, graph, config.max_cut_edges) {
            finding.suggested_fix = Some(RefactorSuggestion::BreakCycle { cut_edges });
        }
    }
}

/// Tries removing 1, then 2 edges from the induced subgraph until the
/// cycle breaks. Only 1- and 2-edge cuts are attempted regardless of a
/// higher `max_cut_edges` config value — a "surgical" cut rarely needs
/// more, and this keeps the search bounded and simple.
fn find_cut_edges(
    cycle: &[String],
    graph: &GraphReport,
    max_cut_edges: usize,
) -> Option<Vec<(String, String)>> {
    let cycle_set = cycle.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let induced_edges = graph
        .edges
        .iter()
        .filter(|edge| {
            cycle_set.contains(edge.from.as_str()) && cycle_set.contains(edge.to.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if induced_edges.is_empty() {
        return None;
    }

    let cycle_files = graph
        .files
        .iter()
        .filter(|file| cycle_set.contains(file.path.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    for i in 0..induced_edges.len() {
        let remaining = without_indices(&induced_edges, &[i]);
        if find_cycles(&cycle_files, &remaining).is_empty() {
            return Some(vec![edge_pair(&induced_edges[i])]);
        }
    }

    if max_cut_edges.min(2) >= 2 {
        for i in 0..induced_edges.len() {
            for j in (i + 1)..induced_edges.len() {
                let remaining = without_indices(&induced_edges, &[i, j]);
                if find_cycles(&cycle_files, &remaining).is_empty() {
                    let mut cut = vec![edge_pair(&induced_edges[i]), edge_pair(&induced_edges[j])];
                    cut.sort();
                    return Some(cut);
                }
            }
        }
    }

    None
}

fn without_indices(edges: &[DependencyEdge], skip: &[usize]) -> Vec<DependencyEdge> {
    edges
        .iter()
        .enumerate()
        .filter(|(index, _)| !skip.contains(index))
        .map(|(_, edge)| edge.clone())
        .collect()
}

fn edge_pair(edge: &DependencyEdge) -> (String, String) {
    (edge.from.clone(), edge.to.clone())
}

#[cfg(test)]
mod tests {
    use super::annotate;
    use crate::config::RefactoringConfig;
    use crate::model::{
        DependencyEdge, DependencyKind, GraphReport, OutputMetadata, PolicyFinding, ProjectFile,
    };

    fn project_file(path: &str) -> ProjectFile {
        ProjectFile {
            id: path.to_string(),
            path: path.to_string(),
            group: None,
            line_count: 10,
        }
    }

    fn edge(from: &str, to: &str) -> DependencyEdge {
        DependencyEdge {
            from: from.to_string(),
            to: to.to_string(),
            kind: DependencyKind::Preload,
            line: 1,
        }
    }

    #[test]
    fn suggests_the_single_edge_that_breaks_a_two_file_cycle() {
        let graph = GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: 2,
            files: vec![project_file("a.gd"), project_file("b.gd")],
            edges: vec![edge("a.gd", "b.gd"), edge("b.gd", "a.gd")],
            cycles: vec![vec!["a.gd".to_string(), "b.gd".to_string()]],
            parser_warnings: vec![],
            file_communities: vec![],
            file_graph_metrics: None,
            entity_scan: None,
        };
        let mut findings = vec![PolicyFinding {
            rule_id: "cycle_detected".to_string(),
            severity: "warning".to_string(),
            message: "dependency cycle detected: a.gd -> b.gd".to_string(),
            file: Some("a.gd".to_string()),
            line: None,
            related_files: vec!["a.gd".to_string(), "b.gd".to_string()],
            suggested_fix: None,
        }];

        annotate(&mut findings, &graph, &RefactoringConfig::default());

        let suggestion = findings[0]
            .suggested_fix
            .as_ref()
            .expect("suggestion present");
        match suggestion {
            crate::model::RefactorSuggestion::BreakCycle { cut_edges } => {
                assert_eq!(cut_edges.len(), 1);
            }
            _ => panic!("expected BreakCycle suggestion"),
        }
    }

    #[test]
    fn skips_cycles_larger_than_max_cycle_files() {
        let files = (0..10)
            .map(|i| project_file(&format!("f{i}.gd")))
            .collect::<Vec<_>>();
        let cycle = files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
        let graph = GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: files.len(),
            files,
            edges: vec![],
            cycles: vec![cycle.clone()],
            parser_warnings: vec![],
            file_communities: vec![],
            file_graph_metrics: None,
            entity_scan: None,
        };
        let mut findings = vec![PolicyFinding {
            rule_id: "cycle_detected".to_string(),
            severity: "warning".to_string(),
            message: "big cycle".to_string(),
            file: None,
            line: None,
            related_files: cycle,
            suggested_fix: None,
        }];

        let config = RefactoringConfig {
            max_cycle_files: 8,
            ..Default::default()
        };
        annotate(&mut findings, &graph, &config);

        assert!(findings[0].suggested_fix.is_none());
    }
}
