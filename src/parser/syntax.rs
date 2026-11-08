//! Shared helpers over gdsyntax's lossless tree: where a node starts and
//! ends in lines, and how to read a string literal.

use gdsyntax::ast::Expr;
use gdsyntax::literal::LiteralValue;
use gdsyntax::{SyntaxKind, SyntaxNode, SyntaxToken, SyntaxTree};

/// Tokens that take part in the code: not comments, whitespace or blank
/// lines, and not the zero-width `INDENT`/`DEDENT` or the statement-ending
/// `NEWLINE`.
pub(crate) fn is_code_token(token: &SyntaxToken) -> bool {
    let kind = token.kind();
    !kind.is_trivia()
        && !matches!(
            kind,
            SyntaxKind::NEWLINE | SyntaxKind::INDENT | SyntaxKind::DEDENT | SyntaxKind::EOF
        )
        && !token.text().is_empty()
}

fn code_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    node.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(is_code_token)
}

/// The line of a byte offset, from `SyntaxTree::position`.
pub(crate) fn line_at(tree: &SyntaxTree, offset: usize) -> usize {
    tree.position(offset).line
}

pub(crate) fn token_line(tree: &SyntaxTree, token: &SyntaxToken) -> usize {
    line_at(tree, usize::from(token.text_range().start()))
}

/// The line of the declaration's own first token, skipping annotations on
/// lines of their own (`@export`, `@rpc`) and leading trivia.
pub(crate) fn declaration_start_line(tree: &SyntaxTree, node: &SyntaxNode) -> Option<usize> {
    node.children_with_tokens()
        .find_map(|element| match element.as_node() {
            Some(child) if child.kind() == SyntaxKind::ANNOTATION_NODE => None,
            Some(child) => code_tokens(child).next(),
            None => element.into_token().filter(is_code_token),
        })
        .map(|token| token_line(tree, &token))
}

/// The line of the first code token in the node.
pub(crate) fn first_code_line(tree: &SyntaxTree, node: &SyntaxNode) -> Option<usize> {
    code_tokens(node)
        .next()
        .map(|token| token_line(tree, &token))
}

/// The line the node's last code token ends on. Trailing comments and blank
/// lines, which the tree keeps as trivia, do not count.
pub(crate) fn last_code_line(tree: &SyntaxTree, node: &SyntaxNode) -> Option<usize> {
    // A code token never ends with a line break, so the line of its end
    // offset is the line of its last character.
    code_tokens(node)
        .last()
        .map(|token| line_at(tree, usize::from(token.text_range().end())))
}

/// The value of a plain or `&"name"` string literal.
pub(crate) fn string_literal(expr: &Expr) -> Option<String> {
    let Expr::Literal(literal) = expr else {
        return None;
    };
    match literal.value()? {
        LiteralValue::String(value) | LiteralValue::StringName(value) => Some(value),
        _ => None,
    }
}

pub(crate) fn is_identifier_like(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The first syntax error as a line and a parser-warning message, if the
/// file has any. gdhealth still measures the rest of the file (see the
/// README).
pub(crate) fn syntax_error_warning(tree: &SyntaxTree) -> Option<(usize, String)> {
    let errors = tree.errors();
    let first = errors.first()?;
    let more = match errors.len() {
        1 => String::new(),
        count => format!(" (+{} more)", count - 1),
    };
    Some((
        first.start.line,
        format!(
            "syntax error: {}{more}; code the parser could not read is left out of this file's metrics",
            first.message
        ),
    ))
}

/// The source text of an expression with its whitespace and line breaks
/// collapsed, for showing a receiver such as `get_node("hud")`.
pub(crate) fn compact_text(node: &SyntaxNode) -> String {
    code_tokens(node)
        .map(|token| token.text().to_string())
        .collect::<String>()
}

/// A receiver chain such as `a.b.c`, or `None` for `self`.
pub(crate) fn receiver_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::SelfExpr(_) => None,
        other => Some(compact_text(other.syntax())),
    }
}
