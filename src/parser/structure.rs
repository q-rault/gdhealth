//! Functions, property accessors, inner classes and top-level symbols, read
//! from gdsyntax's typed tree.
//!
//! Complexity is McCabe's: one plus one per decision. Decisions are `if`,
//! `elif`, `while`, `for`, each `and`/`or` (`&&`/`||`), each ternary
//! `a if c else b`, each `when` guard, and each match pattern except a
//! catch-all (`_` or a lone `var x`). `else` and `match` itself are not
//! decisions. Nesting depth counts the blocks of `if`/`elif`/`else`,
//! `for`, `while` and `match`, inline ones (`if x: return`) included; a
//! lambda's body belongs to the function around it and adds no level.

use crate::parser::syntax::{declaration_start_line, last_code_line};
use gdsyntax::ast::{AstNode, BinaryOp, Expr, Member, Pattern, Stmt, Suite};
use gdsyntax::{SyntaxKind, SyntaxNode, SyntaxTree};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionKind {
    /// A `func` declaration, at file level or in an inner class.
    Func,
    /// The `set(value):` or `get:` body of a property.
    Accessor,
}

#[derive(Debug, Clone)]
pub(crate) struct FunctionShape {
    pub kind: FunctionKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub branch_count: usize,
    pub max_nesting_depth: usize,
    pub parameter_count: usize,
}

/// Every `func` in the file, inner classes included, and every property
/// accessor with a body, in source order.
pub(crate) fn functions(tree: &SyntaxTree) -> Vec<FunctionShape> {
    let mut shapes = Vec::new();
    collect_functions(tree, tree.file().members(), &mut shapes);
    shapes.sort_by_key(|shape| shape.start_line);
    shapes
}

fn collect_functions(
    tree: &SyntaxTree,
    members: impl Iterator<Item = Member>,
    shapes: &mut Vec<FunctionShape>,
) {
    for member in members {
        match member {
            Member::Func(func) => {
                let name = func.name().map_or_else(
                    || "<anonymous>".to_string(),
                    |token| token.text().to_string(),
                );
                let parameter_count = func.params().map_or(0, |list| list.params().count());
                if let Some(shape) = shape_of(
                    tree,
                    FunctionKind::Func,
                    name,
                    func.syntax(),
                    func.body(),
                    parameter_count,
                ) {
                    shapes.push(shape);
                }
            }
            Member::Var(var) => {
                let Some(property) = var.property() else {
                    continue;
                };
                let var_name = var
                    .name()
                    .map_or_else(String::new, |token| token.text().to_string());
                if let Some(setter) = property.setter()
                    && let Some(body) = setter.body()
                    && let Some(shape) = shape_of(
                        tree,
                        FunctionKind::Accessor,
                        format!("{var_name}.set"),
                        setter.syntax(),
                        Some(body),
                        setter.params().map_or(0, |list| list.params().count()),
                    )
                {
                    shapes.push(shape);
                }
                if let Some(getter) = property.getter()
                    && let Some(body) = getter.body()
                    && let Some(shape) = shape_of(
                        tree,
                        FunctionKind::Accessor,
                        format!("{var_name}.get"),
                        getter.syntax(),
                        Some(body),
                        0,
                    )
                {
                    shapes.push(shape);
                }
            }
            Member::Class(class) => {
                if let Some(body) = class.body() {
                    collect_functions(tree, body.members(), shapes);
                }
            }
            _ => {}
        }
    }
}

fn shape_of(
    tree: &SyntaxTree,
    kind: FunctionKind,
    name: String,
    node: &SyntaxNode,
    body: Option<Suite>,
    parameter_count: usize,
) -> Option<FunctionShape> {
    let start_line = declaration_start_line(tree, node)?;
    let end_line = last_code_line(tree, node)
        .unwrap_or(start_line)
        .max(start_line);
    let mut walk = Complexity::default();
    if let Some(body) = body {
        walk.node(body.syntax(), 0);
    }
    Some(FunctionShape {
        kind,
        name,
        start_line,
        end_line,
        branch_count: walk.decisions,
        max_nesting_depth: walk.max_depth,
        parameter_count,
    })
}

#[derive(Default)]
struct Complexity {
    decisions: usize,
    max_depth: usize,
}

impl Complexity {
    /// Visit every statement and expression under `node`, which sits at
    /// nesting `depth`.
    fn node(&mut self, node: &SyntaxNode, depth: usize) {
        for child in node.children() {
            if let Some(stmt) = Stmt::cast(child.clone()) {
                self.stmt(&stmt, depth);
            } else if let Some(expr) = Expr::cast(child.clone()) {
                self.expr(&expr, depth);
            } else {
                self.node(&child, depth);
            }
        }
    }

    fn block(&mut self, body: Option<Suite>, depth: usize) {
        self.max_depth = self.max_depth.max(depth);
        if let Some(body) = body {
            self.node(body.syntax(), depth);
        }
    }

    fn condition(&mut self, condition: Option<Expr>, depth: usize) {
        self.decisions += 1;
        if let Some(condition) = condition {
            self.expr(&condition, depth);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, depth: usize) {
        match stmt {
            Stmt::If(if_stmt) => {
                self.condition(if_stmt.condition(), depth);
                self.block(if_stmt.body(), depth + 1);
                for elif in if_stmt.elif_clauses() {
                    self.condition(elif.condition(), depth);
                    self.block(elif.body(), depth + 1);
                }
                if let Some(else_clause) = if_stmt.else_clause() {
                    self.block(else_clause.body(), depth + 1);
                }
            }
            Stmt::While(while_stmt) => {
                self.condition(while_stmt.condition(), depth);
                self.block(while_stmt.body(), depth + 1);
            }
            Stmt::For(for_stmt) => {
                self.condition(for_stmt.iterable(), depth);
                self.block(for_stmt.body(), depth + 1);
            }
            Stmt::Match(match_stmt) => {
                if let Some(subject) = match_stmt.subject() {
                    self.expr(&subject, depth);
                }
                self.max_depth = self.max_depth.max(depth + 1);
                for branch in match_stmt.branches() {
                    for pattern in branch.patterns() {
                        if !is_catch_all(&pattern) {
                            self.decisions += 1;
                        }
                        // `and`/`or` in an expression pattern still count.
                        self.node(pattern.syntax(), depth + 1);
                    }
                    if let Some(guard) = branch.guard() {
                        self.condition(guard.condition(), depth + 1);
                    }
                    self.block(branch.body(), depth + 1);
                }
            }
            other => self.node(other.syntax(), depth),
        }
    }

    fn expr(&mut self, expr: &Expr, depth: usize) {
        match expr {
            Expr::Binary(binary) if matches!(binary.op(), Some(BinaryOp::And | BinaryOp::Or)) => {
                self.decisions += 1;
            }
            Expr::Ternary(_) => self.decisions += 1,
            _ => {}
        }
        self.node(expr.syntax(), depth);
    }
}

fn is_catch_all(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::Wildcard(_) | Pattern::Bind(_))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolKind {
    Function,
    InnerClass,
    Signal,
    Property,
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolSpan {
    #[allow(dead_code)] // read by tests and reserved for future weighting
    pub kind: SymbolKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    /// How often each identifier appears in the symbol's code; comments
    /// and strings are not code.
    pub identifiers: BTreeMap<String, usize>,
}

/// File-level functions, inner classes, signals and `var`/`const`
/// properties: the substrate for the Split File refactoring suggestion.
pub(crate) fn top_level_symbol_spans(tree: &SyntaxTree) -> Vec<SymbolSpan> {
    tree.file()
        .members()
        .filter_map(|member| {
            let kind = match &member {
                Member::Func(_) => SymbolKind::Function,
                Member::Class(_) => SymbolKind::InnerClass,
                Member::Signal(_) => SymbolKind::Signal,
                Member::Var(_) | Member::Const(_) => SymbolKind::Property,
                _ => return None,
            };
            let name = member.name()?.text().to_string();
            if name.is_empty() {
                return None;
            }
            let node = member.syntax();
            let start_line = declaration_start_line(tree, node)?;
            let end_line = last_code_line(tree, node)
                .unwrap_or(start_line)
                .max(start_line);
            Some(SymbolSpan {
                kind,
                name,
                start_line,
                end_line,
                identifiers: identifier_counts(node),
            })
        })
        .collect()
}

fn identifier_counts(node: &SyntaxNode) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for token in node
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::IDENTIFIER)
    {
        *counts.entry(token.text().to_string()).or_insert(0) += 1;
    }
    counts
}
