//! Individual test cases and mock set-up sites, for the semantic test
//! tasks (`test_value`, `mock_scope`, `test_duplicate`).
//!
//! `skip_ratio` only counts tests; these tasks need each case's span so
//! its body can be judged on its own. Recognised shapes:
//!
//! - Rust: `fn` items carrying a `#[test]` / `#[<runtime>::test]` attribute.
//! - Python: `def test_*` (module level or in a class).
//! - JS / TS: `it(...)` / `test(...)` calls (and `.only` / `.each` forms).
//! - Go: `func TestXxx(t *testing.T)`.
//! - Scala: `test("...") { ... }` (`AnyFunSuite`).

use std::ops::Range;

use tree_sitter::Node;

use crate::observer::code::complexity::ParsedFile;
use crate::observer::shared::lang::Language;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// Function name, or the title string for call-style frameworks.
    pub name: String,
    pub byte_range: Range<usize>,
    /// 1-based, inclusive.
    pub start_line: u32,
    pub end_line: u32,
    pub skipped: bool,
}

fn line_of(row: usize) -> u32 {
    u32::try_from(row + 1).unwrap_or(u32::MAX)
}

fn case_from(node: Node<'_>, name: String, skipped: bool) -> TestCase {
    TestCase {
        name,
        byte_range: node.start_byte()..node.end_byte(),
        start_line: line_of(node.start_position().row),
        end_line: line_of(node.end_position().row),
        skipped,
    }
}

fn walk(node: Node<'_>, f: &mut dyn FnMut(Node<'_>)) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, f);
    }
}

/// Every test case in `parsed`, in source order.
#[must_use]
pub fn test_cases(parsed: &ParsedFile) -> Vec<TestCase> {
    let mut out = match parsed.lang {
        #[cfg(feature = "lang-rust")]
        Language::Rust => rust_cases(parsed),
        #[cfg(feature = "lang-python")]
        Language::Python => python_cases(parsed),
        #[cfg(feature = "lang-typescript")]
        Language::TypeScript | Language::Tsx => jsts_cases(parsed),
        #[cfg(feature = "lang-javascript")]
        Language::JavaScript | Language::Jsx => jsts_cases(parsed),
        #[cfg(feature = "lang-go")]
        Language::Go => go_cases(parsed),
        #[cfg(feature = "lang-scala")]
        Language::Scala => scala_cases(parsed),
    };
    out.sort_by_key(|c| c.byte_range.start);
    out
}

#[cfg(feature = "lang-rust")]
fn rust_cases(parsed: &ParsedFile) -> Vec<TestCase> {
    let src = parsed.source.as_bytes();
    let mut out = Vec::new();
    walk(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "function_item" {
            return;
        }
        // Attributes are preceding siblings of the item they decorate.
        let (mut is_test, mut ignored) = (false, false);
        let mut prev = node.prev_named_sibling();
        while let Some(p) = prev {
            if p.kind() != "attribute_item" {
                break;
            }
            let text = p.utf8_text(src).unwrap_or("");
            let inner = text.trim_start_matches("#[").trim_end_matches(']');
            let path = inner.split(['(', '=']).next().unwrap_or("").trim();
            if path == "test" || path.ends_with("::test") {
                is_test = true;
            }
            if path == "ignore" {
                ignored = true;
            }
            prev = p.prev_named_sibling();
        }
        if !is_test {
            return;
        }
        let name = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(src).ok())
            .unwrap_or("<anonymous>")
            .to_owned();
        out.push(case_from(node, name, ignored));
    });
    out
}

#[cfg(feature = "lang-python")]
fn python_cases(parsed: &ParsedFile) -> Vec<TestCase> {
    let src = parsed.source.as_bytes();
    let mut out = Vec::new();
    walk(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "function_definition" {
            return;
        }
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(src).ok())
        else {
            return;
        };
        if !name.starts_with("test") {
            return;
        }
        let (span, skipped) = match node.parent() {
            Some(p) if p.kind() == "decorated_definition" => {
                let text = p.utf8_text(src).unwrap_or("");
                let head = &text[..text.find("def ").unwrap_or(0)];
                (
                    p,
                    head.contains(".skip") || head.contains(".expectedFailure"),
                )
            }
            _ => (node, false),
        };
        out.push(case_from(span, name.to_owned(), skipped));
    });
    out
}

#[cfg(any(feature = "lang-typescript", feature = "lang-javascript"))]
fn jsts_cases(parsed: &ParsedFile) -> Vec<TestCase> {
    let src = parsed.source.as_bytes();
    let mut out = Vec::new();
    walk(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "call_expression" {
            return;
        }
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        // `it(...)`, `test.only(...)`, and `it.each(table)(...)`.
        let callee_text = callee.utf8_text(src).unwrap_or("");
        let root = callee_text.split(['.', '(']).next().unwrap_or("");
        let skipped = matches!(root, "xit" | "xtest") || callee_text.contains(".skip");
        if !matches!(root, "it" | "test" | "xit" | "xtest" | "fit") {
            return;
        }
        let Some(args) = node.child_by_field_name("arguments") else {
            return;
        };
        let Some(first) = args.named_child(0) else {
            return;
        };
        if !matches!(first.kind(), "string" | "template_string") {
            return;
        }
        let title = first
            .utf8_text(src)
            .unwrap_or("")
            .trim_matches(['"', '\'', '`'])
            .to_owned();
        out.push(case_from(node, title, skipped));
    });
    out
}

#[cfg(feature = "lang-go")]
fn go_cases(parsed: &ParsedFile) -> Vec<TestCase> {
    let src = parsed.source.as_bytes();
    let mut out = Vec::new();
    walk(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "function_declaration" {
            return;
        }
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(src).ok())
        else {
            return;
        };
        let params = node
            .child_by_field_name("parameters")
            .and_then(|p| p.utf8_text(src).ok())
            .unwrap_or("");
        if name.starts_with("Test") && params.contains("testing.T") {
            let body = node
                .child_by_field_name("body")
                .and_then(|b| b.utf8_text(src).ok())
                .unwrap_or("");
            out.push(case_from(node, name.to_owned(), body.contains("t.Skip")));
        }
    });
    out
}

#[cfg(feature = "lang-scala")]
fn scala_cases(parsed: &ParsedFile) -> Vec<TestCase> {
    let src = parsed.source.as_bytes();
    let mut out = Vec::new();
    walk(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "call_expression" {
            return;
        }
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        // `test("title") { body }` parses as a call whose callee is itself
        // the call `test("title")`.
        if callee.kind() != "call_expression" {
            return;
        }
        let Some(inner) = callee.child_by_field_name("function") else {
            return;
        };
        let head = inner.utf8_text(src).unwrap_or("");
        if head != "test" && head != "ignore" {
            return;
        }
        let title = callee
            .child_by_field_name("arguments")
            .and_then(|a| a.named_child(0))
            .and_then(|s| s.utf8_text(src).ok())
            .unwrap_or("")
            .trim_matches('"')
            .to_owned();
        out.push(case_from(node, title, head == "ignore"));
    });
    out
}

/// One mock / stub / spy set-up found by a lexical scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockSite {
    /// 1-based line.
    pub line: u32,
    /// The matched pattern, e.g. `jest.mock(`.
    pub pattern: &'static str,
    /// The trimmed source line.
    pub text: String,
}

fn mock_patterns(lang: Language) -> &'static [&'static str] {
    match lang {
        #[cfg(feature = "lang-rust")]
        Language::Rust => &["#[automock", "mock!", "::default_mock", "expect_", "Mock"],
        #[cfg(feature = "lang-python")]
        Language::Python => &[
            "mock.patch",
            "@patch(",
            "patch.object(",
            "MagicMock(",
            "AsyncMock(",
            "Mock(",
            "mocker.patch",
            "mocker.spy",
            "monkeypatch.setattr",
        ],
        #[cfg(feature = "lang-typescript")]
        Language::TypeScript | Language::Tsx => JS_MOCKS,
        #[cfg(feature = "lang-javascript")]
        Language::JavaScript | Language::Jsx => JS_MOCKS,
        #[cfg(feature = "lang-go")]
        Language::Go => &["gomock.NewController", "NewMock", ".EXPECT()", "mock.Mock"],
        #[cfg(feature = "lang-scala")]
        Language::Scala => &["mock[", "stub[", "spy(", "when("],
    }
}

#[cfg(any(feature = "lang-typescript", feature = "lang-javascript"))]
const JS_MOCKS: &[&str] = &[
    "jest.mock(",
    "vi.mock(",
    "jest.fn(",
    "vi.fn(",
    "jest.spyOn(",
    "vi.spyOn(",
    "sinon.stub(",
    "sinon.mock(",
    "sinon.spy(",
    ".mockReturnValue",
    ".mockResolvedValue",
    ".mockImplementation",
];

/// Lines that set up a mock, stub, or spy. The scan is lexical on
/// purpose: frameworks differ too much for one query per language, and
/// a false positive only costs one extra question. Rust's bare `Mock`
/// pattern requires a `Mock<Upper>` identifier (`MockStore::new()`).
#[must_use]
pub fn mock_sites(source: &str, lang: Language) -> Vec<MockSite> {
    let patterns = mock_patterns(lang);
    let mut out = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('#') && !trimmed.starts_with("#[") {
            continue;
        }
        for &p in patterns {
            let hit = if p == "Mock" {
                trimmed.match_indices("Mock").any(|(i, _)| {
                    let before_ok = i == 0 || !trimmed.as_bytes()[i - 1].is_ascii_alphanumeric();
                    let after = trimmed.as_bytes().get(i + 4);
                    before_ok && after.is_some_and(u8::is_ascii_uppercase)
                })
            } else {
                trimmed.contains(p)
            };
            if hit {
                out.push(MockSite {
                    line: line_of(idx),
                    pattern: p,
                    text: trimmed.to_owned(),
                });
                break;
            }
        }
    }
    out
}

#[cfg(test)]
// Every test here is gated on one grammar; single-language builds
// (`invariants.md` R15) compile the module with none of them.
#[allow(unused_imports)]
mod tests {
    use super::*;
    use crate::observer::code::complexity::parse;

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_tests_and_ignored() {
        let src = "#[test]\nfn a() { assert!(true); }\n#[tokio::test]\n#[ignore = \"slow\"]\nasync fn b() {}\nfn helper() {}\n";
        let cases = test_cases(&parse(src.to_owned(), Language::Rust).unwrap());
        let names: Vec<_> = cases.iter().map(|c| (c.name.as_str(), c.skipped)).collect();
        assert_eq!(names, [("a", false), ("b", true)]);
        assert_eq!((cases[0].start_line, cases[0].end_line), (2, 2));
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn python_tests_including_decorated() {
        let src = "def test_a():\n    assert 1\n\n@pytest.mark.skip\ndef test_b():\n    pass\n\ndef helper():\n    pass\n";
        let cases = test_cases(&parse(src.to_owned(), Language::Python).unwrap());
        let names: Vec<_> = cases.iter().map(|c| (c.name.as_str(), c.skipped)).collect();
        assert_eq!(names, [("test_a", false), ("test_b", true)]);
    }

    #[cfg(feature = "lang-typescript")]
    #[test]
    fn js_call_style_tests() {
        let src = "describe('s', () => {\n  it('adds', () => { expect(1).toBe(1); });\n  test.skip('later', () => {});\n  xit('old', () => {});\n});\n";
        let cases = test_cases(&parse(src.to_owned(), Language::TypeScript).unwrap());
        let names: Vec<_> = cases.iter().map(|c| (c.name.as_str(), c.skipped)).collect();
        assert_eq!(names, [("adds", false), ("later", true), ("old", true)]);
    }

    #[cfg(feature = "lang-go")]
    #[test]
    fn go_tests() {
        let src = "package x\nimport \"testing\"\nfunc TestA(t *testing.T) {}\nfunc TestB(t *testing.T) { t.Skip(\"x\") }\nfunc helper() {}\n";
        let cases = test_cases(&parse(src.to_owned(), Language::Go).unwrap());
        let names: Vec<_> = cases.iter().map(|c| (c.name.as_str(), c.skipped)).collect();
        assert_eq!(names, [("TestA", false), ("TestB", true)]);
    }

    #[cfg(feature = "lang-typescript")]
    #[test]
    fn js_mock_sites() {
        let src =
            "jest.mock('./db');\nconst f = jest.fn();\n// jest.fn() in a comment\nconst x = 1;\n";
        let sites = mock_sites(src, Language::TypeScript);
        assert_eq!(sites.iter().map(|s| s.line).collect::<Vec<_>>(), [1, 2]);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_mock_identifier_needs_upper_suffix() {
        let src = "let m = MockStore::new();\nlet mockery = 1;\nlet x = Mocked;\n";
        let sites = mock_sites(src, Language::Rust);
        assert_eq!(sites.iter().map(|s| s.line).collect::<Vec<_>>(), [1]);
    }
}
