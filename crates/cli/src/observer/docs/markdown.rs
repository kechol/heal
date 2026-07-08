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

/// Replace every backtick-quoted inline-code span (single, double,
/// or triple) on `line` with spaces of equal length, leaving prose
/// outside the spans untouched and column positions unchanged.
/// Single- and double-backtick spans behave the same way — both
/// delimit inline code in `CommonMark`; the doubled form lets
/// authors embed literal backticks. An unclosed backtick run at
/// end-of-line strips to the line end. Returns `Cow::Borrowed`
/// when the line has no backticks so the common case avoids any
/// allocation.
#[must_use]
pub(crate) fn strip_inline_code(line: &str) -> std::borrow::Cow<'_, str> {
    if !line.contains('`') {
        return std::borrow::Cow::Borrowed(line);
    }
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let run_start = i;
        let mut run_len = 0;
        while i < bytes.len() && bytes[i] == b'`' {
            run_len += 1;
            i += 1;
        }
        // Search for a matching closing backtick run of the same length.
        let body_start = i;
        let mut close_pos: Option<usize> = None;
        while i < bytes.len() {
            if bytes[i] != b'`' {
                i += 1;
                continue;
            }
            let mut closing_len = 0;
            let close_start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                closing_len += 1;
                i += 1;
            }
            if closing_len == run_len {
                close_pos = Some(close_start);
                break;
            }
        }
        if let Some(close_start) = close_pos {
            let total = close_start + run_len - run_start;
            out.extend(std::iter::repeat_n(b' ', total));
        } else {
            let trailing = bytes.len() - body_start;
            out.extend(std::iter::repeat_n(b' ', run_len + trailing));
            break;
        }
    }
    // Bytes-only ASCII substitution (spaces) preserves UTF-8 boundaries.
    std::borrow::Cow::Owned(String::from_utf8(out).expect("strip preserves UTF-8 boundaries"))
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
/// relative; the result stays project-relative. A leading `/` is
/// root-relative (GitHub renders `[x](/CONTRIBUTING.md)` from the
/// repo root, not the referring doc's directory); SSG deploy-path
/// links that don't exist in the repo are the province of
/// `exclude_link_prefixes`, not this resolver.
#[must_use]
pub(crate) fn resolve_relative(doc_path: &Path, target: &str) -> PathBuf {
    let parent = if target.starts_with('/') {
        PathBuf::new()
    } else {
        doc_path.parent().map(Path::to_path_buf).unwrap_or_default()
    };
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
    fn strip_inline_code_replaces_single_and_double_backtick_spans() {
        let line = "see `TODO` and ``FIXME`` markers";
        let stripped = strip_inline_code(line);
        // `TODO` (6 bytes) + ``FIXME`` (9 bytes) → equal-length runs
        // of spaces; surrounding prose untouched.
        assert_eq!(stripped, "see        and           markers");
        assert_eq!(stripped.len(), line.len());
        assert!(!stripped.contains("TODO"));
        assert!(!stripped.contains("FIXME"));
    }

    #[test]
    fn strip_inline_code_preserves_text_outside_spans() {
        let line = "real TODO followed by `inline` only";
        let stripped = strip_inline_code(line);
        assert!(stripped.contains("real TODO"));
        assert!(!stripped.contains("inline"));
    }

    #[test]
    fn strip_inline_code_handles_unclosed_backtick() {
        let line = "open `TODO never closes";
        let stripped = strip_inline_code(line);
        assert!(!stripped.contains("TODO"));
        assert_eq!(stripped.len(), line.len());
    }

    #[test]
    fn strip_inline_code_borrows_when_no_backticks() {
        let line = "plain prose with no markup at all";
        let stripped = strip_inline_code(line);
        assert!(matches!(stripped, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn strip_inline_code_keeps_utf8_outside_spans() {
        let line = "[要確認] but not `[要確認]`";
        let stripped = strip_inline_code(line);
        assert!(stripped.contains("[要確認]"));
        assert_eq!(stripped.matches("[要確認]").count(), 1);
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

    #[test]
    fn resolve_relative_treats_leading_slash_as_project_root() {
        // GitHub-style root-relative link: resolves from the repo
        // root, not the referring doc's directory.
        let from = Path::new("docs/sub/page.md");
        assert_eq!(
            resolve_relative(from, "/CONTRIBUTING.md"),
            PathBuf::from("CONTRIBUTING.md")
        );
        assert_eq!(
            resolve_relative(from, "/docs/other.md"),
            PathBuf::from("docs/other.md")
        );
    }
}
