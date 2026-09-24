//! Split a Markdown body into heading-delimited sections.
//!
//! The semantic doc tasks judge one section at a time (docjev's "page",
//! applied to Markdown): a section runs from an ATX heading to the line
//! before the next heading of any level. Content before the first
//! heading forms a level-0 preamble. Headings inside fenced code blocks
//! are ignored. Setext headings (`===` / `---` underlines) are not
//! recognised; the docs HEAL targets use ATX headings.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// 1–6 for `#`..`######`, 0 for the preamble before any heading.
    pub level: u8,
    /// Heading text without the `#` markers or a trailing `#` run.
    pub title: String,
    /// 1-based, inclusive. `start_line` is the heading line itself.
    pub start_line: u32,
    pub end_line: u32,
    /// The section's full text, heading included.
    pub text: String,
}

impl Section {
    /// True when the section has nothing but its heading and blank lines.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text
            .lines()
            .skip(usize::from(self.level > 0))
            .all(|l| l.trim().is_empty())
    }
}

fn heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None; // indented code block
    }
    let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim_end().to_owned();
    Some((u8::try_from(hashes).unwrap_or(6), title))
}

/// Split `body` into sections. Front matter (`---` … `---` on the first
/// line) belongs to the preamble. An empty preamble is dropped.
#[must_use]
pub fn sections(body: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut current = Section {
        level: 0,
        title: String::new(),
        start_line: 1,
        end_line: 0,
        text: String::new(),
    };
    let mut in_fence = false;
    let mut in_front_matter = false;
    for (idx, line) in body.lines().enumerate() {
        let line_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let trimmed = line.trim_start();
        if idx == 0 && line.trim_end() == "---" {
            in_front_matter = true;
        } else if in_front_matter && line.trim_end() == "---" {
            in_front_matter = false;
        } else if !in_front_matter && (trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            in_fence = !in_fence;
        } else if !in_fence && !in_front_matter {
            if let Some((level, title)) = heading(line) {
                let done = std::mem::replace(
                    &mut current,
                    Section {
                        level,
                        title,
                        start_line: line_no,
                        end_line: line_no,
                        text: String::new(),
                    },
                );
                if done.level > 0 || !done.is_empty() {
                    out.push(done);
                }
            }
        }
        current.text.push_str(line);
        current.text.push('\n');
        current.end_line = line_no;
    }
    if current.level > 0 || !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_every_heading_level_and_ignores_fences() {
        let body = "intro\n\n# Title\ntext\n```sh\n# not a heading\n```\n## Sub ##\nmore\n";
        let s = sections(body);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].level, s[0].start_line, s[0].end_line), (0, 1, 2));
        assert_eq!((s[1].level, s[1].title.as_str()), (1, "Title"));
        assert_eq!((s[1].start_line, s[1].end_line), (3, 7));
        assert!(s[1].text.contains("# not a heading"));
        assert_eq!(
            (s[2].level, s[2].title.as_str(), s[2].end_line),
            (2, "Sub", 9)
        );
    }

    #[test]
    fn front_matter_and_empty_preamble() {
        let body = "---\ntitle: x\n---\n# A\nbody\n";
        let s = sections(body);
        assert_eq!(s.len(), 2, "front matter is a non-empty preamble");
        assert_eq!(s[1].title, "A");
        let s = sections("\n\n# A\n");
        assert_eq!(s.len(), 1);
        assert!(s[0].is_empty());
    }

    #[test]
    fn rejects_non_headings() {
        assert!(heading("#hashtag").is_none());
        assert!(heading("    # code").is_none());
        assert!(heading("####### seven").is_none());
        assert_eq!(heading("### x").unwrap().0, 3);
    }
}
