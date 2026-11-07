use crate::error::{GdHealthError, Result};
use globset::Glob;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub refactoring: RefactoringConfig,
    #[serde(default)]
    pub duplication: DuplicationConfig,
}

impl Config {
    pub fn load(explicit_path: Option<&Path>) -> Result<Self> {
        let path = explicit_path
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("gdhealth.toml"));

        if !path.exists() {
            if explicit_path.is_some() {
                return Err(GdHealthError::ReadFile {
                    path,
                    source: std::io::Error::from(std::io::ErrorKind::NotFound),
                });
            }
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).map_err(|source| GdHealthError::ReadFile {
            path: path.clone(),
            source,
        })?;

        let config: Self = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        validate_globs("discovery.include", &self.discovery.include)?;
        validate_globs("discovery.exclude", &self.discovery.exclude)?;

        for (group_name, patterns) in &self.groups {
            validate_globs(&format!("groups.{group_name}"), patterns)?;
        }

        validate_globs("policy.allow_cycles", &self.policy.allow_cycles)?;

        for rule in &self.policy.forbidden_dependencies {
            validate_severity(&rule.severity)?;
        }

        validate_threshold_pair(
            "thresholds.file_lines",
            self.thresholds.file_lines_warn,
            self.thresholds.file_lines_critical,
        )?;
        validate_threshold_pair(
            "thresholds.function_lines",
            self.thresholds.function_lines_warn,
            self.thresholds.function_lines_critical,
        )?;
        validate_threshold_pair(
            "thresholds.cyclomatic",
            self.thresholds.cyclomatic_warn,
            self.thresholds.cyclomatic_critical,
        )?;
        validate_threshold_pair(
            "thresholds.nesting",
            self.thresholds.nesting_warn,
            self.thresholds.nesting_critical,
        )?;

        Ok(())
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    #[serde(default = "default_git_enabled")]
    pub enabled: bool,
    #[serde(default = "default_blame_max_file_lines")]
    pub blame_max_file_lines: usize,
    #[serde(default = "default_hotspot_halflife_days")]
    pub hotspot_halflife_days: f64,
    #[serde(default = "default_co_change_window_days")]
    pub co_change_window_days: f64,
    #[serde(default = "default_co_change_max_files_per_commit")]
    pub co_change_max_files_per_commit: usize,
    #[serde(default = "default_prior_defect_window_days")]
    pub prior_defect_window_days: i64,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            enabled: default_git_enabled(),
            blame_max_file_lines: default_blame_max_file_lines(),
            hotspot_halflife_days: default_hotspot_halflife_days(),
            co_change_window_days: default_co_change_window_days(),
            co_change_max_files_per_commit: default_co_change_max_files_per_commit(),
            prior_defect_window_days: default_prior_defect_window_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactoringConfig {
    #[serde(default = "default_max_cycle_files")]
    pub max_cycle_files: usize,
    #[serde(default = "default_max_cut_edges")]
    pub max_cut_edges: usize,
    #[serde(default = "default_split_file_min_symbols")]
    pub split_file_min_symbols: usize,
    #[serde(default = "default_split_file_modularity_gate")]
    pub split_file_modularity_gate: f64,
}

impl Default for RefactoringConfig {
    fn default() -> Self {
        Self {
            max_cycle_files: default_max_cycle_files(),
            max_cut_edges: default_max_cut_edges(),
            split_file_min_symbols: default_split_file_min_symbols(),
            split_file_modularity_gate: default_split_file_modularity_gate(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicationConfig {
    #[serde(default = "default_duplication_enabled")]
    pub enabled: bool,
    #[serde(default = "default_min_token_window")]
    pub min_token_window: usize,
}

impl Default for DuplicationConfig {
    fn default() -> Self {
        Self {
            enabled: default_duplication_enabled(),
            min_token_window: default_min_token_window(),
        }
    }
}

fn default_git_enabled() -> bool {
    true
}

fn default_blame_max_file_lines() -> usize {
    2000
}

fn default_hotspot_halflife_days() -> f64 {
    60.0
}

fn default_co_change_window_days() -> f64 {
    90.0
}

fn default_co_change_max_files_per_commit() -> usize {
    20
}

fn default_prior_defect_window_days() -> i64 {
    180
}

fn default_max_cycle_files() -> usize {
    8
}

fn default_max_cut_edges() -> usize {
    2
}

fn default_split_file_min_symbols() -> usize {
    4
}

fn default_split_file_modularity_gate() -> f64 {
    0.3
}

fn default_duplication_enabled() -> bool {
    true
}

fn default_min_token_window() -> usize {
    // 40 was too short in practice: idiomatic GDScript field/export
    // boilerplate routinely hits a 40-token match across unrelated files.
    // 150 tokens (roughly a full small function) keeps matches meaningful.
    150
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

fn validate_globs(field: &str, patterns: &[String]) -> Result<()> {
    for pattern in patterns {
        let raw_pattern = pattern.strip_prefix('!').unwrap_or(pattern.as_str());
        Glob::new(raw_pattern).map_err(|error| GdHealthError::InvalidGlob {
            pattern: format!("{field}: {pattern}"),
            message: error.to_string(),
        })?;
    }

    Ok(())
}

fn validate_severity(severity: &str) -> Result<()> {
    match severity.trim().to_ascii_lowercase().as_str() {
        "info" | "warning" | "warn" | "critical" => Ok(()),
        other => Err(GdHealthError::InvalidSeverity {
            severity: other.to_string(),
        }),
    }
}

fn validate_threshold_pair(name: &str, warn: usize, critical: usize) -> Result<()> {
    if warn > critical {
        return Err(GdHealthError::InvalidConfig {
            message: format!("{name} warn threshold {warn} exceeds critical threshold {critical}"),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Config, ForbiddenDependencyRule};
    use crate::error::GdHealthError;

    #[test]
    fn rejects_invalid_forbidden_dependency_severity() {
        let mut config = Config::default();
        config.policy.forbidden_dependencies = vec![ForbiddenDependencyRule {
            from: "server".to_string(),
            to: "hud".to_string(),
            severity: "severe".to_string(),
        }];

        let error = config.validate().expect_err("invalid severity should fail");
        assert!(matches!(error, GdHealthError::InvalidSeverity { .. }));
    }

    #[test]
    fn rejects_invalid_group_glob() {
        let mut config = Config::default();
        config
            .groups
            .insert("broken".to_string(), vec!["[".to_string()]);

        let error = config.validate().expect_err("invalid glob should fail");
        assert!(matches!(error, GdHealthError::InvalidGlob { .. }));
    }

    #[test]
    fn rejects_inverted_thresholds() {
        let mut config = Config::default();
        config.thresholds.cyclomatic_warn = 20;
        config.thresholds.cyclomatic_critical = 10;

        let error = config
            .validate()
            .expect_err("inverted thresholds should fail");
        assert!(matches!(error, GdHealthError::InvalidConfig { .. }));
    }
}
