use crate::model::DependencyKind;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct ParserWarning {
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub class_name: Option<ClassNameDecl>,
    pub dependencies: Vec<DependencyReference>,
    pub warnings: Vec<ParserWarning>,
}

#[derive(Debug, Clone)]
pub struct ClassNameDecl {
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct DependencyReference {
    pub kind: DependencyKind,
    pub target: DependencyTarget,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum DependencyTarget {
    ResourcePath(String),
    NamedClass(String),
}

static CLASS_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:@\w+\s+)*class_name\s+([A-Za-z_][A-Za-z0-9_]*)")
        .expect("valid class_name regex")
});

static EXTENDS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*extends\s+(?:"([^"]+)"|'([^']+)'|([A-Za-z_][A-Za-z0-9_]*))"#)
        .expect("valid extends regex")
});

static PRELOAD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"preload\(\s*(?:"([^"]+)"|'([^']+)')\s*\)"#).expect("valid preload regex")
});

static LOAD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[^A-Za-z_])load\(\s*(?:"([^"]+)"|'([^']+)')\s*\)"#)
        .expect("valid load regex")
});

pub fn parse_file(_path: &str, content: &str) -> ParsedFile {
    let mut class_name = None;
    let mut dependencies = Vec::new();
    let warnings = Vec::new();
    let mut const_initializer_balance = 0isize;

    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        let code = strip_comments(line);
        let trimmed = code.trim_start();
        let starts_const = trimmed.starts_with("const ");
        let in_const_initializer = starts_const || const_initializer_balance > 0;

        if class_name.is_none() {
            if let Some(captures) = CLASS_NAME_RE.captures(code) {
                class_name = captures.get(1).map(|name| ClassNameDecl {
                    name: name.as_str().to_string(),
                    line: line_number,
                });
            }
        }

        if let Some(captures) = EXTENDS_RE.captures(code) {
            if let Some(resource_path) = captures.get(1).or_else(|| captures.get(2)) {
                dependencies.push(DependencyReference {
                    kind: DependencyKind::Extends,
                    target: DependencyTarget::ResourcePath(resource_path.as_str().to_string()),
                    line: line_number,
                });
            } else if let Some(class_name_match) = captures.get(3) {
                dependencies.push(DependencyReference {
                    kind: DependencyKind::Extends,
                    target: DependencyTarget::NamedClass(class_name_match.as_str().to_string()),
                    line: line_number,
                });
            }
        }

        for captures in PRELOAD_RE.captures_iter(code) {
            if let Some(resource_path) = captures.get(1).or_else(|| captures.get(2)) {
                dependencies.push(DependencyReference {
                    kind: if in_const_initializer {
                        DependencyKind::ConstPreload
                    } else {
                        DependencyKind::Preload
                    },
                    target: DependencyTarget::ResourcePath(resource_path.as_str().to_string()),
                    line: line_number,
                });
            }
        }

        for captures in LOAD_RE.captures_iter(code) {
            if let Some(resource_path) = captures.get(1).or_else(|| captures.get(2)) {
                dependencies.push(DependencyReference {
                    kind: DependencyKind::Load,
                    target: DependencyTarget::ResourcePath(resource_path.as_str().to_string()),
                    line: line_number,
                });
            }
        }

        if starts_const {
            const_initializer_balance = code
                .split_once('=')
                .map(|(_, initializer)| delimiter_balance(initializer))
                .unwrap_or(0)
                .max(0);
        } else if const_initializer_balance > 0 {
            const_initializer_balance =
                (const_initializer_balance + delimiter_balance(code)).max(0);
        }
    }

    ParsedFile {
        class_name,
        dependencies,
        warnings,
    }
}

fn strip_comments(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn delimiter_balance(segment: &str) -> isize {
    segment
        .chars()
        .fold(0isize, |balance, character| match character {
            '(' | '[' | '{' => balance + 1,
            ')' | ']' | '}' => balance - 1,
            _ => balance,
        })
}

#[cfg(test)]
mod tests {
    use super::{DependencyTarget, parse_file};
    use crate::model::DependencyKind;

    #[test]
    fn extracts_class_name_and_dependencies() {
        let parsed = parse_file(
            "player.gd",
            r#"
class_name Player
extends "res://base_player.gd"
const HELPER = preload("res://helper.gd")
var dynamic = load(path)
var concrete = load("res://runtime_helper.gd")
"#,
        );

        assert_eq!(
            parsed.class_name.as_ref().map(|item| item.name.as_str()),
            Some("Player")
        );
        assert_eq!(parsed.dependencies.len(), 3);
        assert!(matches!(
            parsed.dependencies[0].kind,
            DependencyKind::Extends
        ));
        assert!(matches!(
            parsed.dependencies[1].target,
            DependencyTarget::ResourcePath(ref path) if path == "res://helper.gd"
        ));
        assert!(matches!(parsed.dependencies[2].kind, DependencyKind::Load));
    }

    #[test]
    fn extracts_named_extends() {
        let parsed = parse_file("child.gd", "extends BaseThing\n");
        assert!(matches!(
            parsed.dependencies[0].target,
            DependencyTarget::NamedClass(ref class_name) if class_name == "BaseThing"
        ));
    }

    #[test]
    fn treats_preloads_inside_multiline_const_initializers_as_const_preloads() {
        let parsed = parse_file(
            "item_catalog.gd",
            r#"
const _ITEMS: Array[ItemProfile] = [
    preload("res://items/amp_tome.tres"),
    preload("res://items/blasting_wand.tres"),
]
"#,
        );

        assert_eq!(parsed.dependencies.len(), 2);
        assert!(
            parsed
                .dependencies
                .iter()
                .all(|dependency| matches!(dependency.kind, DependencyKind::ConstPreload))
        );
    }
}
