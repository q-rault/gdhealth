use crate::error::{GdHealthError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub project: ProjectConfig,
    #[serde(default)]
    pub discovery: DiscoveryConfig,
    #[serde(default)]
    pub groups: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub thresholds: ThresholdConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
}

impl Config {
    pub fn load(explicit_path: Option<&Path>) -> Result<Self> {
        let path = explicit_path
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("gdhealth.toml"));

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).map_err(|source| GdHealthError::ReadFile {
            path: path.clone(),
            source,
        })?;

        Ok(toml::from_str(&content)?)
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut groups = BTreeMap::new();
        groups.insert(
            "root_gameplay".to_string(),
            vec![
                "*.gd".to_string(),
                "!test/**".to_string(),
                "!server/**".to_string(),
                "!client/**".to_string(),
                "!net/**".to_string(),
                "!levels/main_level/hud/**".to_string(),
            ],
        );
        groups.insert("net".to_string(), vec!["net/**".to_string()]);
        groups.insert(
            "ai".to_string(),
            vec!["ai/**".to_string(), "server/ai/**".to_string()],
        );
        groups.insert(
            "hud".to_string(),
            vec![
                "levels/main_level/hud/**".to_string(),
                "levels/main_level/hud.gd".to_string(),
            ],
        );
        groups.insert("server".to_string(), vec!["server/**".to_string()]);
        groups.insert("test".to_string(), vec!["test/**".to_string()]);

        Self {
            project: ProjectConfig::default(),
            discovery: DiscoveryConfig::default(),
            groups,
            thresholds: ThresholdConfig::default(),
            policy: PolicyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default = "default_project_root")]
    pub root: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            root: default_project_root(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    #[serde(default = "default_include")]
    pub include: Vec<String>,
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            include: default_include(),
            exclude: default_exclude(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    #[serde(default = "warn_file_lines")]
    pub file_lines_warn: usize,
    #[serde(default = "critical_file_lines")]
    pub file_lines_critical: usize,
    #[serde(default = "warn_function_lines")]
    pub function_lines_warn: usize,
    #[serde(default = "critical_function_lines")]
    pub function_lines_critical: usize,
    #[serde(default = "warn_cyclomatic")]
    pub cyclomatic_warn: usize,
    #[serde(default = "critical_cyclomatic")]
    pub cyclomatic_critical: usize,
    #[serde(default = "warn_nesting")]
    pub nesting_warn: usize,
    #[serde(default = "critical_nesting")]
    pub nesting_critical: usize,
    #[serde(default = "warn_parameters")]
    pub parameters_warn: usize,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            file_lines_warn: warn_file_lines(),
            file_lines_critical: critical_file_lines(),
            function_lines_warn: warn_function_lines(),
            function_lines_critical: critical_function_lines(),
            cyclomatic_warn: warn_cyclomatic(),
            cyclomatic_critical: critical_cyclomatic(),
            nesting_warn: warn_nesting(),
            nesting_critical: critical_nesting(),
            parameters_warn: warn_parameters(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub forbidden_dependencies: Vec<ForbiddenDependencyRule>,
    #[serde(default)]
    pub allow_cycles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForbiddenDependencyRule {
    pub from: String,
    pub to: String,
    pub severity: String,
}

fn default_project_root() -> String {
    ".".to_string()
}

fn default_include() -> Vec<String> {
    vec!["**/*.gd".to_string()]
}

fn default_exclude() -> Vec<String> {
    vec![
        "addons/**".to_string(),
        ".godot/**".to_string(),
        "reports/**".to_string(),
    ]
}

fn warn_file_lines() -> usize {
    500
}

fn critical_file_lines() -> usize {
    1000
}

fn warn_function_lines() -> usize {
    40
}

fn critical_function_lines() -> usize {
    80
}

fn warn_cyclomatic() -> usize {
    10
}

fn critical_cyclomatic() -> usize {
    15
}

fn warn_nesting() -> usize {
    4
}

fn critical_nesting() -> usize {
    6
}

fn warn_parameters() -> usize {
    5
}
