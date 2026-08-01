//! Rabin-Karp clone detection over the token-KIND stream from
//! `parser::tokenize`. The rolling-hash math is language-agnostic and
//! ported directly; the only GDScript-specific piece is the tokenizer
//! itself. Because identifiers collapse to a single `IDENT` kind, a block
//! that was copy-pasted and then had its variables renamed still matches.

use crate::config::DuplicationConfig;
use crate::isolate::Diagnosable;
use crate::model::{ClonePair, DuplicationReport, GraphReport};
use crate::parser::tokenize::tokenize_kinds;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const HASH_BASE: u128 = 1_099_511_628_211;
const HASH_MOD: u128 = 18_446_744_073_709_551_557; // largest prime below 2^64
// A bucket this large is almost certainly a degenerate repeated pattern
// (e.g. a long run of blank `pass`-only stubs), not a meaningful clone —
// skip it rather than let the O(n^2) pairwise check blow up.
const MAX_BUCKET_SIZE: usize = 200;

struct FileTokens {
    path: String,
    kind_ids: Vec<u64>,
    lines: Vec<usize>,
}

pub fn detect(root: &Path, graph: &GraphReport, config: &DuplicationConfig) -> DuplicationReport {
    if !config.enabled {
        return DuplicationReport::unavailable("duplication detection disabled".to_string());
    }

    let window = config.min_token_window.max(8);
    let files_tokens = tokenize_files(root, graph, window);
    let buckets = build_hash_buckets(&files_tokens, window);
    let clones = collect_clone_pairs(&files_tokens, &buckets, window);

    DuplicationReport {
        available: true,
        unavailable_reason: None,
        clones,
    }
}

fn tokenize_files(root: &Path, graph: &GraphReport, window: usize) -> Vec<FileTokens> {
    let mut kind_ids = BTreeMap::<String, u64>::new();
    let mut next_id = 0u64;
    let mut files_tokens = Vec::new();

    for file in &graph.files {
        if !file.path.ends_with(".gd") {
            continue;
        }
        let Ok(content) = fs::read_to_string(root.join(&file.path)) else {
            continue;
        };
        let tokens = tokenize_kinds(&content);
        if tokens.len() < window {
            continue;
        }

        let mut ids = Vec::with_capacity(tokens.len());
        let mut lines = Vec::with_capacity(tokens.len());
        for token in tokens {
            let id = *kind_ids.entry(token.kind).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            ids.push(id);
            lines.push(token.line);
        }

        files_tokens.push(FileTokens {
            path: file.path.clone(),
            kind_ids: ids,
            lines,
        });
    }

    files_tokens
}

fn build_hash_buckets(
    files_tokens: &[FileTokens],
    window: usize,
) -> BTreeMap<u128, Vec<(usize, usize)>> {
    let mut buckets = BTreeMap::<u128, Vec<(usize, usize)>>::new();
    let base_pow = mod_pow(HASH_BASE, (window - 1) as u64, HASH_MOD);

    for (file_index, file) in files_tokens.iter().enumerate() {
        let n = file.kind_ids.len();
        if n < window {
            continue;
        }

        let mut hash: u128 = 0;
        for &id in &file.kind_ids[0..window] {
            hash = (hash * HASH_BASE + id as u128) % HASH_MOD;
        }
        buckets.entry(hash).or_default().push((file_index, 0));

        for start in 1..=(n - window) {
            let outgoing = file.kind_ids[start - 1] as u128;
            let incoming = file.kind_ids[start + window - 1] as u128;
            hash = (hash + HASH_MOD - (outgoing * base_pow) % HASH_MOD) % HASH_MOD;
            hash = (hash * HASH_BASE + incoming) % HASH_MOD;
            buckets.entry(hash).or_default().push((file_index, start));
        }
    }

    buckets
}

fn collect_clone_pairs(
    files_tokens: &[FileTokens],
    buckets: &BTreeMap<u128, Vec<(usize, usize)>>,
    window: usize,
) -> Vec<ClonePair> {
    let mut clones = Vec::new();
    let mut accepted_ranges = BTreeMap::<(usize, usize), Vec<(usize, usize)>>::new();

    for occurrences in buckets.values() {
        if occurrences.len() < 2 || occurrences.len() > MAX_BUCKET_SIZE {
            continue;
        }

        for i in 0..occurrences.len() {
            for j in (i + 1)..occurrences.len() {
                let (file_a, start_a) = occurrences[i];
                let (file_b, start_b) = occurrences[j];
                if file_a == file_b && start_a.abs_diff(start_b) < window {
                    continue; // trivial self-overlap within the same window
                }

                let slice_a = &files_tokens[file_a].kind_ids[start_a..start_a + window];
                let slice_b = &files_tokens[file_b].kind_ids[start_b..start_b + window];
                if slice_a != slice_b {
                    continue; // hash collision, not a real match
                }

                let (fa, fb, sa, sb) = if file_a <= file_b {
                    (file_a, file_b, start_a, start_b)
                } else {
                    (file_b, file_a, start_b, start_a)
                };

                let entry = accepted_ranges.entry((fa, fb)).or_default();
                let already_covered = entry.iter().any(|&(existing_a, existing_b)| {
                    existing_a.abs_diff(sa) < window && existing_b.abs_diff(sb) < window
                });
                if already_covered {
                    continue;
                }
                entry.push((sa, sb));

                clones.push(ClonePair {
                    file_a: files_tokens[fa].path.clone(),
                    start_line_a: files_tokens[fa].lines[sa],
                    file_b: files_tokens[fb].path.clone(),
                    start_line_b: files_tokens[fb].lines[sb],
                    token_count: window,
                });
            }
        }
    }

    clones.sort_by(|left, right| {
        left.file_a
            .cmp(&right.file_a)
            .then_with(|| left.file_b.cmp(&right.file_b))
            .then_with(|| left.start_line_a.cmp(&right.start_line_a))
            .then_with(|| left.start_line_b.cmp(&right.start_line_b))
    });
    clones
}

fn mod_pow(mut base: u128, mut exp: u64, modulus: u128) -> u128 {
    let mut result = 1u128;
    base %= modulus;
    while exp > 0 {
        if exp & 1 == 1 {
            result = (result * base) % modulus;
        }
        exp >>= 1;
        base = (base * base) % modulus;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::detect;
    use crate::config::DuplicationConfig;
    use crate::model::{GraphReport, OutputMetadata, ProjectFile};

    fn write_project_file(root: &std::path::Path, path: &str, content: &str) -> ProjectFile {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&full, content).expect("write fixture file");
        ProjectFile {
            id: path.to_string(),
            path: path.to_string(),
            group: None,
            line_count: content.lines().count(),
        }
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("gdhealth-duplication-{prefix}-{}", std::process::id()))
    }

    const DUPLICATED_BLOCK: &str = r#"func award_xp(amount: int) -> void:
    if amount <= 0:
        return
    var total = current_xp + amount
    if total >= xp_to_next_level:
        total -= xp_to_next_level
        level += 1
        xp_to_next_level = compute_next_threshold(level)
    current_xp = total
    emit_signal("xp_changed", current_xp)
"#;

    #[test]
    fn detects_a_clone_even_with_renamed_identifiers() {
        let root = temp_dir("renamed");
        let renamed_block = DUPLICATED_BLOCK
            .replace("award_xp", "grant_xp")
            .replace("amount", "value")
            .replace("total", "sum");

        let file_a = write_project_file(&root, "a.gd", DUPLICATED_BLOCK);
        let file_b = write_project_file(&root, "b.gd", &renamed_block);

        let graph = GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: 2,
            files: vec![file_a, file_b],
            edges: vec![],
            cycles: vec![],
            parser_warnings: vec![],
            file_communities: vec![],
            file_graph_metrics: None,
            entity_scan: None,
        };

        let mut config = DuplicationConfig::default();
        config.min_token_window = 20;
        let report = detect(&root, &graph, &config);

        assert!(report.available);
        assert!(
            !report.clones.is_empty(),
            "renamed clone should still be detected"
        );
        let pair = &report.clones[0];
        assert_eq!(pair.file_a, "a.gd");
        assert_eq!(pair.file_b, "b.gd");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn does_not_report_unrelated_files_as_clones() {
        let root = temp_dir("unrelated");
        let file_a = write_project_file(&root, "a.gd", DUPLICATED_BLOCK);
        let file_b = write_project_file(
            &root,
            "b.gd",
            "extends RefCounted\n\nfunc greet(name: String) -> String:\n    return \"hi \" + name\n",
        );

        let graph = GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: 2,
            files: vec![file_a, file_b],
            edges: vec![],
            cycles: vec![],
            parser_warnings: vec![],
            file_communities: vec![],
            file_graph_metrics: None,
            entity_scan: None,
        };

        let mut config = DuplicationConfig::default();
        config.min_token_window = 20;
        let report = detect(&root, &graph, &config);

        assert!(report.available);
        assert!(report.clones.is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn disabled_config_reports_unavailable() {
        let root = temp_dir("disabled");
        std::fs::create_dir_all(&root).ok();
        let graph = GraphReport {
            metadata: OutputMetadata::default(),
            root: ".".to_string(),
            file_count: 0,
            files: vec![],
            edges: vec![],
            cycles: vec![],
            parser_warnings: vec![],
            file_communities: vec![],
            file_graph_metrics: None,
            entity_scan: None,
        };
        let mut config = DuplicationConfig::default();
        config.enabled = false;
        let report = detect(&root, &graph, &config);
        assert!(!report.available);

        std::fs::remove_dir_all(&root).ok();
    }
}
