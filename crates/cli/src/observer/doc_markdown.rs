//! Shared Markdown / RST helpers for the `[features.docs]` family.
//!
//! Five observers (`doc_drift`, `doc_link_health`, `orphan_pages`,
//! `todo_density`, plus `duplication`'s Markdown pass) all need the
//! same fence-aware line scan, the same identifier / link parser, the
//! same external-URL classifier. Keeping the logic in one place stops
//! the family from drifting (a pre-extraction `is_external` check
//! had already gone out of sync between two observers).

use std::path::{Path, PathBuf};

/// Iterate `body` line-by-line, skipping content inside fenced code
/// blocks (` ``` `, ` ~~~ `). Yields `(1-based-line-number, line)`
/// pairs so callers can record finding locations without re-counting.
pub(crate) fn iter_prose_lines(body: &str) -> ProseLines<'_> {
    ProseLines {
        lines: body.lines().enumerate(),
        in_fence: false,
    }
}

pub(crate) struct ProseLines<'a> {
    lines: std::iter::Enumerate<std::str::Lines<'a>>,
    in_fence: bool,
}

impl<'a> Iterator for ProseLines<'a> {
    type Item = (u32, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        for (idx, line) in self.lines.by_ref() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                self.in_fence = !self.in_fence;
                continue;
            }
            if self.in_fence {
                continue;
            }
            return Some((u32::try_from(idx + 1).unwrap_or(u32::MAX), line));
        }
        None
    }
}

/// One Markdown `[text](target)` link. `line` is 1-based; `target` is
/// the raw value with the optional title stripped (`(./x.md "title")`
/// → `./x.md`).
#[derive(Debug, Clone)]
pub(crate) struct LinkRef {
    pub target: String,
    pub line: u32,
}

/// Extract every `[text](target)` link from a Markdown body, skipping
/// fenced code blocks and image-style `![alt](src)` references.
pub(crate) fn extract_links(body: &str) -> Vec<LinkRef> {
    let mut out: Vec<LinkRef> = Vec::new();
    for (line_no, line) in iter_prose_lines(body) {
        scan_line(line, line_no, &mut out);
    }
    out
}

fn scan_line(line: &str, line_no: u32, out: &mut Vec<LinkRef>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        if i > 0 && (bytes[i - 1] == b'\\' || bytes[i - 1] == b'!') {
            i += 1;
            continue;
        }
        let Some(close_text) = find_unescaped(bytes, i + 1, b']') else {
            return;
        };
        if close_text + 1 >= bytes.len() || bytes[close_text + 1] != b'(' {
            i = close_text + 1;
            continue;
        }
        let Some(close_target) = find_unescaped(bytes, close_text + 2, b')') else {
            return;
        };
        let raw = &line[close_text + 2..close_target];
        // Markdown's `[text](target "title")` — keep only the URL.
        let target = raw.split_whitespace().next().unwrap_or(raw);
        out.push(LinkRef {
            target: target.to_owned(),
            line: line_no,
        });
        i = close_target + 1;
    }
}

fn find_unescaped(bytes: &[u8], from: usize, target: u8) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == target {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// True iff the link target points outside the doc graph. Used by
/// every Layer B observer to skip externals — HTTP/HTTPS link
/// checking is `scope.md` R5 out-of-scope.
#[must_use]
pub(crate) fn is_external(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("ftp://")
        || target.starts_with("//")
}

/// Split `path#anchor` into `(path, anchor)`. Either may be empty.
#[must_use]
pub(crate) fn split_link_target(target: &str) -> (&str, &str) {
    target.split_once('#').unwrap_or((target, ""))
}

/// Resolve `target` against the directory containing `doc_path`,
/// collapsing `./` and `../` segments. Both inputs are project-
/// relative; the result stays project-relative.
#[must_use]
pub(crate) fn resolve_relative(doc_path: &Path, target: &str) -> PathBuf {
    let parent = doc_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out: Vec<&str> = parent
        .iter()
        .filter_map(|os| os.to_str())
        .filter(|s| !s.is_empty())
        .collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_prose_lines_skips_fenced_blocks() {
        let body = "alpha\n```\nbravo\n```\ncharlie\n~~~\ndelta\n~~~\necho\n";
        let lines: Vec<(u32, &str)> = iter_prose_lines(body).collect();
        assert_eq!(lines, vec![(1, "alpha"), (5, "charlie"), (9, "echo")]);
    }

    #[test]
    fn extract_links_recognises_relative_paths_and_anchors() {
        let body =
            "see [docs](./other.md) and [api](#section).\n\n```\n[skip](inside-fence)\n```\n";
        let links = extract_links(body);
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, vec!["./other.md", "#section"]);
        assert_eq!(links[0].line, 1);
    }

    #[test]
    fn is_external_covers_common_schemes() {
        for s in ["http://x", "https://x", "mailto:a@b", "ftp://x", "//cdn"] {
            assert!(is_external(s), "{s} should be external");
        }
        for s in ["./local.md", "#anchor", "../other.md"] {
            assert!(!is_external(s), "{s} should be internal");
        }
    }

    #[test]
    fn split_link_target_round_trips() {
        assert_eq!(split_link_target("./x.md#anchor"), ("./x.md", "anchor"));
        assert_eq!(split_link_target("./x.md"), ("./x.md", ""));
        assert_eq!(split_link_target("#anchor"), ("", "anchor"));
    }

    #[test]
    fn resolve_relative_collapses_dotdot() {
        let from = Path::new("docs/sub/page.md");
        assert_eq!(
            resolve_relative(from, "./sibling.md"),
            PathBuf::from("docs/sub/sibling.md")
        );
        assert_eq!(
            resolve_relative(from, "../other.md"),
            PathBuf::from("docs/other.md")
        );
        assert_eq!(
            resolve_relative(from, "deep/nested.md"),
            PathBuf::from("docs/sub/deep/nested.md")
        );
    }
}
