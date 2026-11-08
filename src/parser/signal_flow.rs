use crate::model::{SignalOperation, UnresolvedSignalReference};
use crate::parser::syntax::{first_code_line, is_identifier_like, receiver_text, string_literal};
use gdsyntax::SyntaxTree;
use gdsyntax::ast::{AstNode, CallExpr, Expr, Member};
use std::collections::BTreeSet;

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
    analyze_signal_flow_tree(file, &gdsyntax::parse(content))
}

/// Signals the script declares, and where it emits or connects them.
///
/// Recognised forms: `emit_signal("name")`, `connect("name", ...)`,
/// `name.emit(...)`, `name.connect(...)`, and the same with a receiver
/// (`other.name.emit()`, `other.connect("name", ...)`). `self` as a receiver
/// is the script itself. Strings and comments are never read as code.
pub fn analyze_signal_flow_tree(file: &str, tree: &SyntaxTree) -> SignalFlowAnalysis {
    let declarations = tree
        .file()
        .members()
        .filter_map(|member| match member {
            Member::Signal(signal) => signal.name().map(|name| name.text().to_string()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let declared = declarations
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let usages = tree
        .root()
        .descendants()
        .filter_map(CallExpr::cast)
        .filter_map(|call| {
            let (operation, signal, receiver) = signal_usage(&call)?;
            Some(Usage {
                line: first_code_line(tree, call.syntax()).unwrap_or(1),
                operation,
                signal,
                receiver,
            })
        })
        .collect::<Vec<_>>();
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

fn operation_of(method: &str) -> Option<SignalOperation> {
    match method {
        "emit" | "emit_signal" => Some(SignalOperation::Emit),
        "connect" => Some(SignalOperation::Connect),
        _ => None,
    }
}

/// The operation, signal and receiver of a call that emits or connects a
/// signal.
fn signal_usage(call: &CallExpr) -> Option<(SignalOperation, String, Option<String>)> {
    let named_signal = || {
        call.arguments()
            .next()
            .as_ref()
            .and_then(string_literal)
            .filter(|name| is_identifier_like(name))
    };
    match call.callee()? {
        // `emit_signal("name")`, `connect("name", callable)`
        Expr::Name(name) => {
            let method = name.name()?;
            if !matches!(method.text(), "emit_signal" | "connect") {
                return None;
            }
            Some((operation_of(method.text())?, named_signal()?, None))
        }
        Expr::Attribute(attribute) => {
            let method = attribute.name()?;
            let operation = operation_of(method.text())?;
            let base = attribute.base()?;
            // `receiver.emit_signal("name")`, `receiver.connect("name", ...)`
            if method.text() != "emit"
                && let Some(signal) = named_signal()
            {
                return Some((operation, signal, receiver_text(&base)));
            }
            if method.text() == "emit_signal" {
                return None;
            }
            // `name.emit()`, `receiver.name.connect(callable)`
            match base {
                Expr::Name(name) => Some((operation, name.name()?.text().to_string(), None)),
                Expr::Attribute(signal) => {
                    let receiver = signal.base()?;
                    Some((
                        operation,
                        signal.name()?.text().to_string(),
                        receiver_text(&receiver),
                    ))
                }
                _ => None,
            }
        }
        _ => None,
    }
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

    #[test]
    fn reads_self_receivers_multiline_calls_and_call_receivers() {
        let flow = analyze_signal_flow(
            "hud.gd",
            "signal opened\nfunc _ready():\n    self.opened.emit()\n    self.connect(\n        \"opened\",\n        _on_opened\n    )\n    _health.emit_signal(\"hp_changed\", 1)\n    get_tree().process_frame.connect(_tick)\n    var help = \"\"\"\n    opened.emit()\n    \"\"\"\n",
        );
        assert_eq!((flow.emit_count, flow.connection_count), (1, 1));
        let unresolved = flow
            .unresolved_references
            .iter()
            .map(|r| (r.line, r.signal.as_deref(), r.receiver.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            unresolved,
            vec![
                (8, Some("hp_changed"), Some("_health")),
                (9, Some("process_frame"), Some("get_tree()")),
            ]
        );
    }
}
