use crate::model::DependencyKind;
use crate::parser::syntax::{first_code_line, string_literal, syntax_error_warning, token_line};
use gdsyntax::ast::{AstNode, CallExpr, Expr, Extends, PreloadExpr};
use gdsyntax::{SyntaxKind, SyntaxToken, SyntaxTree};

#[derive(Debug, Clone)]
pub struct ParserWarning {
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
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

pub fn parse_file(path: &str, content: &str) -> ParsedFile {
    let tree = gdsyntax::parse(content);
    parse_tree(path, &tree)
}

/// `class_name`, `extends`, `preload(...)` and `load(...)` from the typed
/// tree. Strings and comments are never read as code, and calls that span
/// lines are found.
pub fn parse_tree(path: &str, tree: &SyntaxTree) -> ParsedFile {
    let file = tree.file();
    let class_name = file.class_name().and_then(|declaration| {
        let name = declaration.name()?;
        Some(ClassNameDecl {
            name: name.text().to_string(),
            line: token_line(tree, &name),
        })
    });

    let mut dependencies = Vec::new();
    for node in tree.root().descendants() {
        if let Some(extends) = Extends::cast(node.clone()) {
            dependencies.extend(extends_dependency(tree, &extends));
        } else if let Some(preload) = PreloadExpr::cast(node.clone()) {
            let Some(resource_path) = preload.path().as_ref().and_then(string_literal) else {
                continue;
            };
            let in_const = node
                .ancestors()
                .any(|ancestor| ancestor.kind() == SyntaxKind::CONST);
            dependencies.push(DependencyReference {
                kind: if in_const {
                    DependencyKind::ConstPreload
                } else {
                    DependencyKind::Preload
                },
                target: DependencyTarget::ResourcePath(resource_path),
                line: first_code_line(tree, &node).unwrap_or(1),
            });
        } else if let Some(call) = CallExpr::cast(node.clone()) {
            let Some(name) = callee_name(&call).filter(|name| name.text() == "load") else {
                continue;
            };
            let Some(resource_path) = call.arguments().next().as_ref().and_then(string_literal)
            else {
                continue;
            };
            dependencies.push(DependencyReference {
                kind: DependencyKind::Load,
                target: DependencyTarget::ResourcePath(resource_path),
                line: token_line(tree, &name),
            });
        }
    }
    dependencies.sort_by_key(|dependency| (dependency.line, kind_rank(&dependency.kind)));

    let warnings = syntax_error_warning(tree)
        .map(|(line, message)| ParserWarning {
            file: path.to_string(),
            line,
            message,
        })
        .into_iter()
        .collect();

    ParsedFile {
        class_name,
        dependencies,
        warnings,
    }
}

/// `extends` of the script, written alone or after `class_name`, and of
/// inner classes, whether `extends` is on the `class` line or in its body.
fn extends_dependency(tree: &SyntaxTree, extends: &Extends) -> Option<DependencyReference> {
    let line = first_code_line(tree, extends.syntax()).unwrap_or(1);
    let target = if let Some(path) = extends.path() {
        DependencyTarget::ResourcePath(gdsyntax::literal::string_value(path.text())?)
    } else {
        DependencyTarget::NamedClass(extends.names().next()?.text().to_string())
    };
    Some(DependencyReference {
        kind: DependencyKind::Extends,
        target,
        line,
    })
}

/// `load` in `load(...)` or `ResourceLoader.load(...)`.
fn callee_name(call: &CallExpr) -> Option<SyntaxToken> {
    match call.callee()? {
        Expr::Name(name) => name.name(),
        Expr::Attribute(attribute) => attribute.name(),
        _ => None,
    }
}

fn kind_rank(kind: &DependencyKind) -> u8 {
    match kind {
        DependencyKind::Extends => 0,
        DependencyKind::Preload | DependencyKind::ConstPreload => 1,
        DependencyKind::Load => 2,
        DependencyKind::UnknownScriptReference => 3,
    }
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

    #[test]
    fn reads_multiline_calls_and_inline_class_name_extends() {
        let parsed = parse_file(
            "hud.gd",
            "class_name Hud extends \"res://base_hud.gd\"\n\nconst SLOT: PackedScene = preload(\n\t\"res://slot.tscn\"\n)\nvar help = \"preload('res://not_code.gd')\"\n# load(\"res://commented.gd\")\n\nfunc _ready():\n\tvar theme = ResourceLoader.load(\"res://theme.tres\")\n\tvar icon = preload(\"res://icon.png\")\n\nclass Inner extends BaseThing:\n\tpass\n",
        );
        assert_eq!(
            parsed
                .class_name
                .as_ref()
                .map(|item| (item.name.as_str(), item.line)),
            Some(("Hud", 1))
        );
        let dependencies = parsed
            .dependencies
            .iter()
            .map(|dependency| {
                let target = match &dependency.target {
                    DependencyTarget::ResourcePath(path) => path.clone(),
                    DependencyTarget::NamedClass(name) => name.clone(),
                };
                (format!("{:?}", dependency.kind), target, dependency.line)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            dependencies,
            vec![
                ("Extends".to_string(), "res://base_hud.gd".to_string(), 1),
                ("ConstPreload".to_string(), "res://slot.tscn".to_string(), 3),
                ("Load".to_string(), "res://theme.tres".to_string(), 10),
                ("Preload".to_string(), "res://icon.png".to_string(), 11),
                ("Extends".to_string(), "BaseThing".to_string(), 13),
            ]
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn warns_about_syntax_errors_and_keeps_the_rest() {
        let parsed = parse_file(
            "broken.gd",
            "extends Node\nconst A = preload(\"res://a.gd\")\nfunc broken(:\n\tpass\nconst B = preload(\"res://b.gd\")\n",
        );
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.warnings[0].line, 3);
        assert!(parsed.warnings[0].message.starts_with("syntax error: "));
        assert!(parsed.dependencies.iter().any(|dependency| matches!(
            dependency.target,
            DependencyTarget::ResourcePath(ref path) if path == "res://b.gd"
        )));
    }
}
