use super::exec::{GitError, run_git_text};
use std::collections::HashMap;
use std::path::Path;

const RECORD_SEP: char = '\u{1e}';
const UNIT_SEP: char = '\u{1f}';

#[derive(Debug, Clone)]
pub struct FileDelta {
    pub path: String,
    pub added: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone)]
pub struct CommitRecord {
    pub hash: String,
    pub unix_time: i64,
    pub author_email: String,
    pub subject: String,
    pub files: Vec<FileDelta>,
}

/// Walks the full commit history in two passes joined by hash: one for
/// metadata (author/time/subject, using ASCII record/unit separators so
/// commit subjects can never corrupt the split), one for `--numstat`
/// line deltas. Two passes are simpler and more robust to parse correctly
/// than interleaving a rich `--format` with `--numstat` in one call.
pub fn walk_commits(root: &Path) -> Result<Vec<CommitRecord>, GitError> {
    let metadata_format = format!("--format={RECORD_SEP}%H{UNIT_SEP}%at{UNIT_SEP}%ae{UNIT_SEP}%s");
    let metadata_text = run_git_text(root, &["log", "--no-merges", &metadata_format])?;
    let mut records = parse_metadata(&metadata_text);

    let numstat_format = format!("--format={RECORD_SEP}%H");
    let numstat_text = run_git_text(
        root,
        &["log", "--no-merges", &numstat_format, "--numstat"],
    )?;
    let deltas_by_hash = parse_numstat(&numstat_text);

    for record in &mut records {
        if let Some(files) = deltas_by_hash.get(&record.hash) {
            record.files = files.clone();
        }
    }

    Ok(records)
}

fn parse_metadata(text: &str) -> Vec<CommitRecord> {
    text.split(RECORD_SEP)
        .filter(|chunk| !chunk.trim().is_empty())
        .filter_map(|chunk| {
            let chunk = chunk.trim_end_matches('\n');
            let mut parts = chunk.splitn(4, UNIT_SEP);
            let hash = parts.next()?.to_string();
            let unix_time = parts.next()?.parse::<i64>().ok()?;
            let author_email = parts.next()?.to_string();
            let subject = parts.next().unwrap_or("").trim_end().to_string();
            Some(CommitRecord {
                hash,
                unix_time,
                author_email,
                subject,
                files: Vec::new(),
            })
        })
        .collect()
}

fn parse_numstat(text: &str) -> HashMap<String, Vec<FileDelta>> {
    let mut map = HashMap::new();

    for chunk in text.split(RECORD_SEP) {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }

        let mut lines = chunk.lines();
        let Some(hash) = lines.next() else { continue };
        let hash = hash.trim();
        if hash.is_empty() {
            continue;
        }

        let mut deltas = Vec::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.splitn(3, '\t');
            let added = fields.next().unwrap_or("0");
            let deleted = fields.next().unwrap_or("0");
            let path = fields.next().unwrap_or("").trim().to_string();
            if path.is_empty() {
                continue;
            }
            // Binary files report `-` for added/deleted; treat as zero churn.
            deltas.push(FileDelta {
                path,
                added: added.parse().unwrap_or(0),
                deleted: deleted.parse().unwrap_or(0),
            });
        }

        map.insert(hash.to_string(), deltas);
    }

    map
}

#[cfg(test)]
mod tests {
    use super::{parse_metadata, parse_numstat};

    #[test]
    fn parses_metadata_records_separated_by_ascii_control_chars() {
        let text = "\u{1e}abc123\u{1f}1700000000\u{1f}dev@example.com\u{1f}fix: crash\n\u{1e}def456\u{1f}1700003600\u{1f}other@example.com\u{1f}add feature\n";
        let records = parse_metadata(text);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].hash, "abc123");
        assert_eq!(records[0].unix_time, 1_700_000_000);
        assert_eq!(records[0].author_email, "dev@example.com");
        assert_eq!(records[0].subject, "fix: crash");
        assert_eq!(records[1].hash, "def456");
    }

    #[test]
    fn parses_numstat_records_including_binary_dash_markers() {
        let text = "\u{1e}abc123\n3\t1\ta.gd\n-\t-\tbinary.png\n\u{1e}def456\n5\t0\tb.gd\n";
        let map = parse_numstat(text);
        assert_eq!(map["abc123"].len(), 2);
        assert_eq!(map["abc123"][0].added, 3);
        assert_eq!(map["abc123"][0].deleted, 1);
        assert_eq!(map["abc123"][1].added, 0);
        assert_eq!(map["def456"][0].path, "b.gd");
    }
}
