use crate::model::{SignalOperation, UnresolvedSignalReference};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

static DECLARATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^signal\s+([A-Za-z_][A-Za-z0-9_]*)\b").unwrap());
static EMIT_SIGNAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bemit_signal\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']"#).unwrap()
});
static CONNECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bconnect\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']"#).unwrap());
static MEMBER_EMIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)\.emit\s*\(").unwrap());
static MEMBER_CONNECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)\.connect\s*\(").unwrap());
static RECEIVER_MEMBER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\.([A-Za-z_][A-Za-z0-9_]*)\.(emit|connect)\s*\(").unwrap()
});
static RECEIVER_CONNECT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\.connect\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']"#).unwrap()
});

#[derive(Debug, Clone)]
pub struct SignalFlowAnalysis {
    pub declarations: Vec<String>,
    pub emit_count: usize,
    pub connection_count: usize,
    pub unresolved_references: Vec<UnresolvedSignalReference>,
}

#[derive(Debug)]
struct Usage {
    line: usize,
    operation: SignalOperation,
    signal: String,
    receiver: Option<String>,
}

pub fn analyze_signal_flow(file: &str, content: &str) -> SignalFlowAnalysis {
    let lines = content.lines().collect::<Vec<_>>();
    let declarations = lines
        .iter()
        .filter_map(|line| {
            if line.starts_with([' ', '\t']) {
                return None;
            }
            let code = code_before_comment(line);
            DECLARATION_RE
                .captures(code)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let declared = declarations
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut usages = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let line_number = index + 1;
        for c in RECEIVER_MEMBER_RE.captures_iter(line) {
            if is_code_position(line, c.get(0).unwrap().start()) {
                usages.push(Usage {
                    line: line_number,
                    receiver: Some(c[1].to_string()),
                    signal: c[2].to_string(),
                    operation: if &c[3] == "emit" {
                        SignalOperation::Emit
                    } else {
                        SignalOperation::Connect
                    },
                });
            }
        }
        for c in RECEIVER_CONNECT_RE.captures_iter(line) {
            if is_code_position(line, c.get(0).unwrap().start()) {
                usages.push(Usage {
                    line: line_number,
                    receiver: Some(c[1].to_string()),
                    signal: c[2].to_string(),
                    operation: SignalOperation::Connect,
                });
            }
        }
        for (re, operation) in [
            (&*EMIT_SIGNAL_RE, SignalOperation::Emit),
            (&*CONNECT_RE, SignalOperation::Connect),
            (&*MEMBER_EMIT_RE, SignalOperation::Emit),
            (&*MEMBER_CONNECT_RE, SignalOperation::Connect),
        ] {
            for c in re.captures_iter(line) {
                let whole = c.get(0).unwrap();
                if !is_code_position(line, whole.start())
                    || has_receiver_prefix(line, whole.start())
                    || (matches!(operation, SignalOperation::Connect)
                        && whole.as_str().ends_with('(')
                        && line[whole.end()..].trim_start().starts_with(['\'', '\"']))
                {
                    continue;
                }
                usages.push(Usage {
                    line: line_number,
                    receiver: None,
                    signal: c[1].to_string(),
                    operation: operation.clone(),
                });
            }
        }
    }
    let mut emit_count = 0;
    let mut connection_count = 0;
    let mut unresolved_references = Vec::new();
    for usage in usages {
        if usage.receiver.is_none() && declared.contains(usage.signal.as_str()) {
            match usage.operation {
                SignalOperation::Emit => emit_count += 1,
                SignalOperation::Connect => connection_count += 1,
            }
        } else {
            unresolved_references.push(UnresolvedSignalReference {
                file: file.to_string(),
                line: usage.line,
                operation: usage.operation,
                signal: Some(usage.signal),
                receiver: usage.receiver,
            });
        }
    }
    unresolved_references.sort_by(|a, b| {
        (&a.file, a.line, &a.operation, &a.signal, &a.receiver).cmp(&(
            &b.file,
            b.line,
            &b.operation,
            &b.signal,
            &b.receiver,
        ))
    });
    SignalFlowAnalysis {
        declarations,
        emit_count,
        connection_count,
        unresolved_references,
    }
}

fn code_before_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(ch, '\'' | '\"') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
        } else if ch == '#' && quote.is_none() {
            return &line[..index];
        }
    }
    line
}
fn is_code_position(line: &str, position: usize) -> bool {
    code_before_comment(line).len() >= position && {
        let prefix = &line[..position];
        let mut quote = None;
        let mut escaped = false;
        for ch in prefix.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' && quote.is_some() {
                escaped = true;
                continue;
            }
            if matches!(ch, '\'' | '\"') {
                if quote == Some(ch) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(ch);
                }
            }
        }
        quote.is_none()
    }
}
fn has_receiver_prefix(line: &str, start: usize) -> bool {
    line[..start].trim_end().ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::analyze_signal_flow;
    #[test]
    fn extracts_local_and_unresolved_signal_flow() {
        let flow = analyze_signal_flow(
            "player.gd",
            "signal health_changed(value: int)\nfunc ready():\n    emit_signal(\"health_changed\")\n    health_changed.emit()\n    connect(\"health_changed\", callback)\n    health_changed.connect(callback)\n    other.health_changed.emit()\n    other.connect(\"health_changed\", callback)\n    print(\"health_changed.emit()\") # connect(\"health_changed\")\n    signal ignored\n",
        );
        assert_eq!(flow.declarations, vec!["health_changed"]);
        assert_eq!((flow.emit_count, flow.connection_count), (2, 2));
        assert_eq!(
            flow.unresolved_references.len(),
            2,
            "{:?}",
            flow.unresolved_references
        );
    }
}
