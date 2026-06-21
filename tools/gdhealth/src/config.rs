use crate::error::{GdHealthError, Result};
use globset::Glob;
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
