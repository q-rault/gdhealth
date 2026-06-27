use crate::cli::CommandOptions;
use crate::config::Config;
use crate::error::{GdHealthError, Result};
use crate::model::ProjectFile;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub fn discover_files(config: &Config, options: &CommandOptions) -> Result<Vec<ProjectFile>> {
    let include_set = compile_globs(&config.discovery.include)?;
    let exclude_set = compile_globs(&config.discovery.exclude)?;
    let root = options.root.as_path();
    let mut files = Vec::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        let relative = normalize_path(path.strip_prefix(root).unwrap_or(path));

        if !include_set.is_match(relative.as_str()) {
            continue;
        }

        if exclude_set.is_match(relative.as_str()) {
            continue;
        }

        if !options.include_tests && (relative == "test" || relative.starts_with("test/")) {
            continue;
        }

        let content = fs::read_to_string(path).map_err(|source| GdHealthError::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;

        let line_count = content.lines().count();
        let group = match_group(&relative, &config.groups)?;

        files.push(ProjectFile {
            id: relative.clone(),
            path: relative,
            group,
            line_count,
        });
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

pub fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn match_group(
    path: &str,
    groups: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Option<String>> {
    for (group_name, patterns) in groups {
        if matches_group_patterns(path, patterns)? {
            return Ok(Some(group_name.clone()));
        }
    }

    Ok(None)
}

fn matches_group_patterns(path: &str, patterns: &[String]) -> Result<bool> {
    let mut included = false;

    for pattern in patterns {
        let is_negated = pattern.starts_with('!');
        let raw_pattern = if is_negated {
            &pattern[1..]
        } else {
            pattern.as_str()
        };
        let glob = Glob::new(raw_pattern).map_err(|error| GdHealthError::InvalidGlob {
            pattern: pattern.clone(),
            message: error.to_string(),
        })?;
        let is_match = glob.compile_matcher().is_match(path);

        if is_negated && is_match {
            return Ok(false);
        }

        if !is_negated && is_match {
            included = true;
        }
    }

    Ok(included)
}

fn compile_globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();

    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|error| GdHealthError::InvalidGlob {
            pattern: pattern.clone(),
            message: error.to_string(),
        })?;
        builder.add(glob);
    }

    builder.build().map_err(|error| GdHealthError::InvalidGlob {
        pattern: patterns.join(", "),
        message: error.to_string(),
    })
}
