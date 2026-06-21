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
    pub signal_count: usize,
    pub public_function_count: usize,
    pub outgoing_dependency_count: usize,
    pub incoming_dependency_count: usize,
    pub cycle_count: usize,
    pub file_hotspot_score: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsReport {
    #[serde(flatten)]
    pub metadata: OutputMetadata,
    pub root: String,
    pub files: Vec<FileMetric>,
    pub functions: Vec<FunctionMetric>,
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
}
