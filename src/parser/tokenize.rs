//! Lightweight token-KIND tokenizer for duplication detection.
//!
//! Deliberately not a real GDScript lexer: it classifies each lexeme into a
//! coarse *kind* (keyword, operator, `IDENT`, `NUM`, `STR`) rather than
//! preserving the source text. Identifiers all collapse to the same `IDENT`
//! kind regardless of name, so a block that was copy-pasted and then had
//! its variables renamed still hashes identically — that's the whole point
//! for clone detection.

const KEYWORDS: &[&str] = &[
    "if", "elif", "else", "for", "while", "func", "var", "const", "class_name", "class",
    "extends", "return", "break", "continue", "pass", "match", "signal", "static", "self",
    "true", "false", "null", "and", "or", "not", "in", "is", "as", "preload", "load", "await",
    "yield", "enum", "export", "onready", "tool", "setget", "void", "super",
];

const MULTI_CHAR_OPS: &[&str] = &[
    "==", "!=", "<=", ">=", "->", ":=", "+=", "-=", "*=", "/=", "%=", "**", "&&", "||",
];

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: String,
    pub line: usize,
}

pub fn tokenize_kinds(content: &str) -> Vec<Token> {
    let chars = content.chars().collect::<Vec<_>>();
    let n = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;

    while i < n {
        let c = chars[i];

        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '#' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            let start_line = line;
            let quote = c;
            i += 1;
            while i < n {
                if chars[i] == '\\' && i + 1 < n {
                    i += 2;
                    continue;
                }
                if chars[i] == '\n' {
                    line += 1;
                }
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(Token {
                kind: "STR".to_string(),
                line: start_line,
            });
            continue;
        }
        if c.is_ascii_digit() {
            let start_line = line;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token {
                kind: "NUM".to_string(),
                line: start_line,
            });
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            let start_line = line;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word = chars[start..i].iter().collect::<String>();
            let kind = if KEYWORDS.contains(&word.as_str()) {
                format!("KW_{}", word.to_ascii_uppercase())
            } else {
                "IDENT".to_string()
            };
            tokens.push(Token {
                kind,
                line: start_line,
            });
            continue;
        }

        if i + 1 < n {
            let two = [c, chars[i + 1]].iter().collect::<String>();
            if MULTI_CHAR_OPS.contains(&two.as_str()) {
                tokens.push(Token { kind: two, line });
                i += 2;
                continue;
            }
        }

        tokens.push(Token {
            kind: c.to_string(),
            line,
        });
        i += 1;
    }

    tokens
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
        assert_eq!(kinds_a, kinds_b, "renamed identifiers must tokenize to the same kind stream");
    }

    #[test]
    fn distinguishes_keywords_and_operators_by_kind() {
        let tokens = tokenize_kinds("if a == b:\n    pass\n");
        let kinds = tokens.iter().map(|t| t.kind.as_str()).collect::<Vec<_>>();
        assert_eq!(kinds, vec!["KW_IF", "IDENT", "==", "IDENT", ":", "KW_PASS"]);
    }

    #[test]
    fn skips_comments_and_strings_collapse_to_one_token() {
        let tokens = tokenize_kinds("# a comment\nvar name = \"hello world\"\n");
        let kinds = tokens.iter().map(|t| t.kind.as_str()).collect::<Vec<_>>();
        assert_eq!(kinds, vec!["KW_VAR", "IDENT", "=", "STR"]);
    }

    #[test]
    fn tracks_line_numbers_across_multiline_strings() {
        let tokens = tokenize_kinds("var a = \"line1\nline2\"\nvar b = 1\n");
        assert_eq!(tokens[0].line, 1); // KW_VAR for `a`
        assert_eq!(tokens[3].kind, "STR");
        assert_eq!(tokens[4].line, 3); // KW_VAR for `b`, after the multiline string
    }
}
