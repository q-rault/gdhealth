use serde::Serialize;

pub const TOOL_NAME: &str = "gdhealth";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct OutputMetadata {
    pub tool_name: &'static str,
    pub tool_version: &'static str,
    pub schema_version: u32,
}

impl Default for OutputMetadata {
    fn default() -> Self {
        Self {
            tool_name: TOOL_NAME,
            tool_version: TOOL_VERSION,
            schema_version: SCHEMA_VERSION,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFile {
    pub id: String,
    pub path: String,
    pub group: Option<String>,
    pub line_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub enum DependencyKind {
    Extends,
    Preload,
    Load,
    ConstPreload,
    UnknownScriptReference,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphReport {
    #[serde(flatten)]
    pub metadata: OutputMetadata,
    pub root: String,
    pub file_count: usize,
    pub files: Vec<ProjectFile>,
    pub edges: Vec<DependencyEdge>,
    pub cycles: Vec<Vec<String>>,
    pub parser_warnings: Vec<String>,
    pub file_communities: Vec<GraphCommunity>,
    pub file_graph_metrics: Option<GraphAggregateMetrics>,
    pub entity_scan: Option<EntityScanReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionMetric {
    pub file: String,
    pub function_name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub line_count: usize,
    pub branch_count: usize,
    pub cyclomatic_estimate: usize,
    pub max_nesting_depth: usize,
    pub parameter_count: usize,
    pub hotspot_score: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileMetric {
    pub file: String,
    pub line_count: usize,
    pub function_count: usize,
    pub number_of_methods: usize,
    pub signal_count: usize,
    pub public_function_count: usize,
    pub outgoing_dependency_count: usize,
    pub incoming_dependency_count: usize,
    pub cycle_count: usize,
    pub ws_complexity: usize,
    pub tfidf: f64,
    pub louvain_modularity: usize,
    pub file_hotspot_score: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsReport {
    #[serde(flatten)]
    pub metadata: OutputMetadata,
    pub root: String,
    pub files: Vec<FileMetric>,
    pub functions: Vec<FunctionMetric>,
    pub entities: Vec<EntityMetric>,
    /// Experimental — token-kind clone detection, not yet a stable API
    /// surface (same posture as `entity_scan` when it was introduced).
    pub duplication: DuplicationReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClonePair {
    pub file_a: String,
    pub start_line_a: usize,
    pub file_b: String,
    pub start_line_b: usize,
    pub token_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicationReport {
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub clones: Vec<ClonePair>,
}

impl crate::isolate::Diagnosable for DuplicationReport {
    fn unavailable(reason: String) -> Self {
        Self {
            available: false,
            unavailable_reason: Some(reason),
            clones: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphCommunity {
    pub id: String,
    pub community_id: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphAggregateMetrics {
    pub louvain_modularity: f64,
    pub community_count: usize,
    pub total_sloc: usize,
    pub group_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityNode {
    pub id: String,
    pub display_name: String,
    pub file: String,
    pub class_name: Option<String>,
    pub extends: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub line_count: usize,
    pub method_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityScanReport {
    pub entities: Vec<EntityNode>,
    pub dependency_edges: Vec<EntityEdge>,
    pub inheritance_edges: Vec<EntityEdge>,
    pub complete_edges: Vec<EntityEdge>,
    pub communities: Vec<GraphCommunity>,
    pub complete_graph_metrics: GraphAggregateMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityMetric {
    pub entity: String,
    pub file: String,
    pub line_count: usize,
    pub method_count: usize,
    pub ws_complexity: usize,
    pub tfidf: f64,
    pub incoming_dependency_count: usize,
    pub outgoing_dependency_count: usize,
    pub incoming_inheritance_count: usize,
    pub outgoing_inheritance_count: usize,
    pub louvain_modularity: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyFinding {
    pub rule_id: String,
    pub severity: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub related_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyReport {
    #[serde(flatten)]
    pub metadata: OutputMetadata,
    pub root: String,
    pub findings: Vec<PolicyFinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthSummary {
    pub file_count: usize,
    pub edge_count: usize,
    pub policy_finding_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    #[serde(flatten)]
    pub metadata: OutputMetadata,
    pub summary: HealthSummary,
    pub top_files: Vec<FileMetric>,
    pub top_functions: Vec<FunctionMetric>,
    pub cycles: Vec<Vec<String>>,
    pub policy_findings: Vec<PolicyFinding>,
    pub cross_group_dependencies: Vec<String>,
    pub git_available: bool,
    pub git_unavailable_reason: Option<String>,
    pub git_shallow_repository: bool,
    pub repo_bus_factor: usize,
    pub top_churn_files: Vec<GitFileStats>,
    pub top_co_changes: Vec<CoChangePair>,
}

// ── Git intelligence (available but not yet a stable API surface, same
// posture as entity_scan when it was introduced) ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct GitOwnershipShare {
    pub author_email: String,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitFileStats {
    pub file: String,
    pub commit_count_total: usize,
    pub commit_count_90d: usize,
    pub commit_count_30d: usize,
    pub lines_added_90d: usize,
    pub lines_deleted_90d: usize,
    pub temporal_hotspot_score: f64,
    pub change_entropy: f64,
    pub prior_defect_count: usize,
    pub ownership: Vec<GitOwnershipShare>,
    pub churn_percentile: f64,
    pub is_churn_hotspot: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoChangePair {
    pub file_a: String,
    pub file_b: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitIntelligenceReport {
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub shallow_repository: bool,
    pub anchor_commit_unix_time: Option<i64>,
    pub repo_bus_factor: usize,
    pub repo_bus_factor_contributors: Vec<String>,
    pub files: Vec<GitFileStats>,
    pub co_change_pairs: Vec<CoChangePair>,
    pub warnings: Vec<String>,
}

impl crate::isolate::Diagnosable for GitIntelligenceReport {
    fn unavailable(reason: String) -> Self {
        Self {
            available: false,
            unavailable_reason: Some(reason),
            shallow_repository: false,
            anchor_commit_unix_time: None,
            repo_bus_factor: 0,
            repo_bus_factor_contributors: Vec::new(),
            files: Vec::new(),
            co_change_pairs: Vec::new(),
            warnings: Vec::new(),
        }
    }
}
