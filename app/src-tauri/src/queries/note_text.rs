//! Text from vault notes: reading the ones a prompt or a suggestion quotes, a one-line gist of a note, and clipping.

use std::collections::HashMap;
use std::path::Path;

/// First `n` characters, on a char boundary, with an ellipsis if cut.
pub fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

/// Vault notes a prompt quotes, read after the snapshot that named them has
/// closed (the database is never held across a file read).
#[derive(Debug, Default)]
pub struct Notes(HashMap<String, String>);

impl Notes {
    pub fn read(vault: &Path, paths: impl IntoIterator<Item = String>) -> Notes {
        Notes(
            paths
                .into_iter()
                .map(|p| (p.clone(), std::fs::read_to_string(vault.join(&p)).unwrap_or_default()))
                .collect(),
        )
    }

    /// The note's text, or "" if it wasn't read or isn't there.
    pub fn get(&self, path: &str) -> &str {
        self.0.get(path).map_or("", String::as_str)
    }
}

/// One-line description of a topic from the first lines of its note
/// (frontmatter and blank lines skipped), for topic suggestions.
pub fn note_gist(note: &str) -> String {
    let mut lines = note.lines().map(str::trim).peekable();
    if lines.peek() == Some(&"---") {
        lines.next();
        for l in lines.by_ref() {
            if l == "---" {
                break;
            }
        }
    }
    let parts: Vec<&str> = lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.trim_start_matches(['#', '-', '*', ' '])
                .trim_start_matches("[ ]")
                .trim_start_matches("[x]")
                .trim()
        })
        .filter(|l| !l.is_empty())
        .take(4)
        .collect();
    clip(&parts.join(" / "), 200)
}

#[cfg(test)]
mod tests;
