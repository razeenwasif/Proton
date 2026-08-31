//! Live grep behind `␣fg`.
//!
//! Deliberately plain: walk the same file list, read each file, report the
//! lines that contain the needle. No regex engine and no parallelism yet —
//! both are easy to add and neither is the thing that makes live grep feel
//! good. What makes it feel good is stopping early, so this takes a limit and
//! honours it, and skipping anything that is not text, so a binary never costs
//! a scan.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct GrepHit {
    pub path: String,
    /// Zero-based, as everywhere else in the core; the UI adds one.
    pub line: usize,
    pub column: usize,
    pub text: String,
}

/// Search `paths` (relative to `root`) for `needle`, case-insensitively.
///
/// Returns at most `limit` hits and stops walking once it has them — a live
/// grep that insists on finding every match in a large tree is a live grep
/// that stutters on the fourth keystroke.
pub fn grep(root: &Path, paths: &[String], needle: &str, limit: usize) -> Vec<GrepHit> {
    if needle.is_empty() {
        return Vec::new();
    }
    let lowered = needle.to_lowercase();
    let mut hits = Vec::new();

    for path in paths {
        if hits.len() >= limit {
            break;
        }
        let Ok(contents) = std::fs::read(root.join(path)) else {
            continue;
        };
        // A NUL in the first block is the same heuristic git uses for "binary".
        if contents.iter().take(8000).any(|&b| b == 0) {
            continue;
        }
        let Ok(text) = String::from_utf8(contents) else {
            continue;
        };

        for (line, content) in text.lines().enumerate() {
            if hits.len() >= limit {
                break;
            }
            let Some(column) = content.to_lowercase().find(&lowered) else {
                continue;
            };
            hits.push(GrepHit {
                path: path.clone(),
                line,
                // `find` gives a byte offset; the rest of the core counts
                // characters, and a hit after an emoji would be off otherwise.
                column: content[..column].chars().count(),
                text: content.trim_end().to_string(),
            });
        }
    }

    hits
}
