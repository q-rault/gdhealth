//! Token-KIND stream for duplication detection.
//!
//! Each GDScript token from gdsyntax's tokenizer is reduced to a coarse
//! *kind* rather than its text: every identifier is `IDENT`, every number
//! `NUM`, every string `STR`, and keywords, operators and punctuation keep
//! their own kind. A block that was copy-pasted and then had its variables
//! renamed still hashes identically — that's the whole point for clone
//! detection. Comments, whitespace, blank lines and indentation are trivia
//! in the lossless tree and are skipped.

use crate::parser::syntax::is_code_token;
use gdsyntax::SyntaxKind;

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: String,
    pub line: usize,
}

pub fn tokenize_kinds(content: &str) -> Vec<Token> {
    // Token-level line numbers come from a line-start table: calling
    // `SyntaxTree::position` per token would rescan the file each time.
    let line_starts = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(index, _)| index + 1))
        .collect::<Vec<_>>();
    let (root, _errors) = gdsyntax::lossless_tree(content);
    root.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(is_code_token)
        .map(|token| {
            let offset = usize::from(token.text_range().start());
            Token {
                kind: kind_name(token.kind()),
                line: line_starts.partition_point(|start| *start <= offset),
            }
        })
        .collect()
}

fn kind_name(kind: SyntaxKind) -> String {
    match kind {
        SyntaxKind::IDENTIFIER => "IDENT".to_string(),
        SyntaxKind::INT | SyntaxKind::FLOAT => "NUM".to_string(),
        SyntaxKind::STRING | SyntaxKind::STRING_NAME | SyntaxKind::NODE_PATH => "STR".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::tokenize_kinds;

    #[test]
    fn collapses_identifiers_to_a_single_kind_regardless_of_name() {
        let a = tokenize_kinds("func heal(target):\n    target.hp += 10\n");
        let b = tokenize_kinds("func revive(entity):\n    entity.hp += 10\n");
        let kinds_a = a.iter().map(|t| t.kind.clone()).collect::<Vec<_>>();
        let kinds_b = b.iter().map(|t| t.kind.clone()).collect::<Vec<_>>();
        assert_eq!(
            kinds_a, kinds_b,
            "renamed identifiers must tokenize to the same kind stream"
        );
    }

    #[test]
    fn distinguishes_keywords_and_operators_by_kind() {
        let tokens = tokenize_kinds("if a == b:\n    pass\n");
        let kinds = tokens.iter().map(|t| t.kind.as_str()).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec!["IF_KW", "IDENT", "EQUAL_EQUAL", "IDENT", "COLON", "PASS_KW"]
        );
    }

    #[test]
    fn skips_comments_and_strings_collapse_to_one_token() {
        let tokens = tokenize_kinds("# a comment\nvar name = \"hello world\"\n");
        let kinds = tokens.iter().map(|t| t.kind.as_str()).collect::<Vec<_>>();
        assert_eq!(kinds, vec!["VAR_KW", "IDENT", "EQUAL", "STR"]);
    }

    #[test]
    fn tracks_line_numbers_across_multiline_strings() {
        let tokens = tokenize_kinds("var a = \"line1\nline2\"\nvar b = 1\n");
        assert_eq!(tokens[0].line, 1); // KW_VAR for `a`
        assert_eq!(tokens[3].kind, "STR");
        assert_eq!(tokens[4].line, 3); // KW_VAR for `b`, after the multiline string
    }
}
