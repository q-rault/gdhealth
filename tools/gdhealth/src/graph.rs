use crate::cli::CommandOptions;
use crate::config::Config;
use crate::discovery::discover_files;
use crate::error::{GdHealthError, Result};
use crate::model::{DependencyEdge, DependencyKind, GraphReport, OutputMetadata, ProjectFile};
use crate::parser::extract::{DependencyReference, DependencyTarget, ParsedFile, parse_file};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub fn build_graph(config: &Config, options: &CommandOptions) -> Result<GraphReport> {
    let files = discover_files(config, options)?;
    let root = options.root.as_path();
    let parsed_files = parse_project_files(root, &files)?;
    let class_index = build_class_index(&parsed_files);
    let mut parser_warnings = collect_class_warnings(&parsed_files, &class_index);
    let mut edges = Vec::new();

    for file in &files {
        if let Some(parsed) = parsed_files.get(&file.path) {
            for dependency in &parsed.dependencies {
                match resolve_dependency(root, &file.path, dependency, &class_index) {
                    Some(target) => edges.push(DependencyEdge {
                        from: file.path.clone(),
                        to: target,
                        kind: dependency.kind.clone(),
                        line: dependency.line,
                    }),
                    None => {
                        if should_warn_unresolved(dependency) {
                            parser_warnings.push(format_unresolved_warning(&file.path, dependency));
                        }
                    }
                }
            }
        }
    }

    edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| dependency_kind_rank(&left.kind).cmp(&dependency_kind_rank(&right.kind)))
    });

    let cycles = find_cycles(&files, &edges);

    Ok(GraphReport {
        metadata: OutputMetadata::default(),
        root: options.root.to_string_lossy().into_owned(),
        file_count: files.len(),
        files,
        edges,
        cycles,
        parser_warnings,
    })
}

fn parse_project_files(root: &Path, files: &[ProjectFile]) -> Result<BTreeMap<String, ParsedFile>> {
    let mut parsed = BTreeMap::new();

    for file in files {
        let absolute_path = root.join(&file.path);
        let content =
            fs::read_to_string(&absolute_path).map_err(|source| GdHealthError::ReadFile {
                path: absolute_path.clone(),
                source,
            })?;
        parsed.insert(file.path.clone(), parse_file(&file.path, &content));
    }

    Ok(parsed)
}

fn build_class_index(
    parsed_files: &BTreeMap<String, ParsedFile>,
) -> BTreeMap<String, Vec<(String, usize)>> {
    let mut index = BTreeMap::<String, Vec<(String, usize)>>::new();

    for (path, parsed) in parsed_files {
        if let Some(class_name) = &parsed.class_name {
            index
                .entry(class_name.name.clone())
                .or_default()
                .push((path.clone(), class_name.line));
        }
    }

    for values in index.values_mut() {
        values.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    }

    index
}

fn collect_class_warnings(
    parsed_files: &BTreeMap<String, ParsedFile>,
    class_index: &BTreeMap<String, Vec<(String, usize)>>,
) -> Vec<String> {
    let mut warnings = Vec::new();

    for (class_name, declarations) in class_index {
        if declarations.len() > 1 {
            let locations = declarations
                .iter()
                .map(|(path, line)| format!("{path}:{line}"))
                .collect::<Vec<_>>()
                .join(", ");
            warnings.push(format!(
                "duplicate class_name `{class_name}` declared at {locations}"
            ));
        }
    }

    for (path, parsed) in parsed_files {
        for warning in &parsed.warnings {
            warnings.push(format!(
                "{}:{}: {}",
                if warning.file.is_empty() {
                    path
                } else {
                    &warning.file
                },
                warning.line,
                warning.message
            ));
        }
    }

    warnings.sort();
    warnings
}

fn resolve_dependency(
    root: &Path,
    from_path: &str,
    dependency: &DependencyReference,
    class_index: &BTreeMap<String, Vec<(String, usize)>>,
) -> Option<String> {
    match &dependency.target {
        DependencyTarget::ResourcePath(path) => resolve_resource_path(root, from_path, path),
        DependencyTarget::NamedClass(class_name) => class_index
            .get(class_name)
            .and_then(|matches| matches.first())
            .map(|entry| entry.0.clone()),
    }
}

fn resolve_resource_path(root: &Path, from_path: &str, target: &str) -> Option<String> {
    let normalized = normalize_resource_reference(target);
    if !normalized.ends_with(".gd") {
        return None;
    }

    let candidate = if target.starts_with("res://") {
        normalized
    } else {
        let base = Path::new(from_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        normalize_relative_path(base, Path::new(target))
    };

    let absolute = root.join(&candidate);
    absolute.exists().then_some(candidate)
}

fn normalize_resource_reference(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("res://") {
        normalize_relative_path(Path::new(""), Path::new(stripped))
    } else {
        normalize_relative_path(Path::new(""), Path::new(path))
    }
}

fn normalize_relative_path(base: &Path, path: &Path) -> String {
    let combined = if path.is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };

    let mut normalized = Vec::<String>::new();
    for component in combined.components() {
        let value = component.as_os_str().to_string_lossy();
        match value.as_ref() {
            "." => {}
            ".." => {
                normalized.pop();
            }
            other => normalized.push(other.to_string()),
        }
    }

    normalized.join("/")
}

fn should_warn_unresolved(dependency: &DependencyReference) -> bool {
    match &dependency.target {
        DependencyTarget::ResourcePath(path) => path.ends_with(".gd"),
        DependencyTarget::NamedClass(_) => false,
    }
}

fn format_unresolved_warning(from_path: &str, dependency: &DependencyReference) -> String {
    match &dependency.target {
        DependencyTarget::ResourcePath(path) => {
            format!(
                "{from_path}:{}: unresolved script path `{path}`",
                dependency.line
            )
        }
        DependencyTarget::NamedClass(class_name) => {
            format!(
                "{from_path}:{}: unresolved class_name reference `{class_name}`",
                dependency.line
            )
        }
    }
}

fn dependency_kind_rank(kind: &DependencyKind) -> usize {
    match kind {
        DependencyKind::Extends => 0,
        DependencyKind::ConstPreload => 1,
        DependencyKind::Preload => 2,
        DependencyKind::Load => 3,
        DependencyKind::UnknownScriptReference => 4,
    }
}

fn find_cycles(files: &[ProjectFile], edges: &[DependencyEdge]) -> Vec<Vec<String>> {
    let known_files = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let mut adjacency = BTreeMap::<String, Vec<String>>::new();

    for file in files {
        adjacency.entry(file.path.clone()).or_default();
    }

    for edge in edges {
        if known_files.contains(&edge.to) {
            adjacency
                .entry(edge.from.clone())
                .or_default()
                .push(edge.to.clone());
        }
    }

    for neighbors in adjacency.values_mut() {
        neighbors.sort();
        neighbors.dedup();
    }

    let mut tarjan = Tarjan::new(adjacency);
    tarjan.run()
}

struct Tarjan {
    adjacency: BTreeMap<String, Vec<String>>,
    next_index: usize,
    indices: BTreeMap<String, usize>,
    lowlinks: BTreeMap<String, usize>,
    stack: Vec<String>,
    on_stack: BTreeSet<String>,
    components: Vec<Vec<String>>,
}

impl Tarjan {
    fn new(adjacency: BTreeMap<String, Vec<String>>) -> Self {
        Self {
            adjacency,
            next_index: 0,
            indices: BTreeMap::new(),
            lowlinks: BTreeMap::new(),
            stack: Vec::new(),
            on_stack: BTreeSet::new(),
            components: Vec::new(),
        }
    }

    fn run(&mut self) -> Vec<Vec<String>> {
        let nodes = self.adjacency.keys().cloned().collect::<Vec<_>>();
        for node in nodes {
            if !self.indices.contains_key(&node) {
                self.visit(node);
            }
        }

        self.components
            .sort_by(|left, right| left[0].cmp(&right[0]));
        self.components.clone()
    }

    fn visit(&mut self, node: String) {
        let index = self.next_index;
        self.next_index += 1;
        self.indices.insert(node.clone(), index);
        self.lowlinks.insert(node.clone(), index);
        self.stack.push(node.clone());
        self.on_stack.insert(node.clone());

        let neighbors = self.adjacency.get(&node).cloned().unwrap_or_default();
        for neighbor in neighbors {
            if !self.indices.contains_key(&neighbor) {
                self.visit(neighbor.clone());
                let neighbor_lowlink = self.lowlinks[&neighbor];
                let current_lowlink = self.lowlinks[&node];
                self.lowlinks
                    .insert(node.clone(), current_lowlink.min(neighbor_lowlink));
            } else if self.on_stack.contains(&neighbor) {
                let neighbor_index = self.indices[&neighbor];
                let current_lowlink = self.lowlinks[&node];
                self.lowlinks
                    .insert(node.clone(), current_lowlink.min(neighbor_index));
            }
        }

        if self.lowlinks[&node] == self.indices[&node] {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack.remove(&member);
                component.push(member.clone());
                if member == node {
                    break;
                }
            }

            component.sort();
            let is_cycle = component.len() > 1
                || self
                    .adjacency
                    .get(&component[0])
                    .map(|neighbors| neighbors.contains(&component[0]))
                    .unwrap_or(false);
            if is_cycle {
                self.components.push(component);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{find_cycles, normalize_relative_path, resolve_resource_path};
    use crate::model::{DependencyEdge, DependencyKind, ProjectFile};
    use std::path::Path;

    #[test]
    fn resolves_relative_resource_paths() {
        let root = std::env::temp_dir().join(format!("gdhealth-graph-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("shared")).expect("create temp fixture");
        std::fs::write(root.join("shared/model.gd"), "extends RefCounted\n")
            .expect("write temp fixture");
        let result = resolve_resource_path(&root, "levels/ui/menu.gd", "../../shared/model.gd");
        assert_eq!(result.as_deref(), Some("shared/model.gd"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_multi_file_cycles() {
        let files = vec![
            ProjectFile {
                id: "a.gd".to_string(),
                path: "a.gd".to_string(),
                group: None,
                line_count: 1,
            },
            ProjectFile {
                id: "b.gd".to_string(),
                path: "b.gd".to_string(),
                group: None,
                line_count: 1,
            },
        ];
        let edges = vec![
            DependencyEdge {
                from: "a.gd".to_string(),
                to: "b.gd".to_string(),
                kind: DependencyKind::Extends,
                line: 1,
            },
            DependencyEdge {
                from: "b.gd".to_string(),
                to: "a.gd".to_string(),
                kind: DependencyKind::Extends,
                line: 1,
            },
        ];

        let cycles = find_cycles(&files, &edges);
        assert_eq!(cycles, vec![vec!["a.gd".to_string(), "b.gd".to_string()]]);
    }

    #[test]
    fn normalizes_paths_without_leaking_parent_segments() {
        let result =
            normalize_relative_path(Path::new("levels/main_level"), Path::new("../hud.gd"));
        assert_eq!(result, "levels/hud.gd");
    }
}
