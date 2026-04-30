//! Language registry — maps file extensions to a tree-sitter grammar plus the
//! per-language compiled query set used by complexity observers.
//!
//! Compiled queries (and their typed capture indices) are cached statically per
//! language variant via `OnceLock`, so callers that analyze thousands of files
//! pay query-compile cost exactly once per (language, query) pair.
//!
//! Variants are gated by `#[non_exhaustive]` so adding `JavaScript` / `Jsx`
//! later doesn't break exhaustive matches downstream. Each variant is also
//! gated behind a Cargo feature (`lang-ts`, `lang-rust`) so a downstream binary
//! can drop unused grammars to shrink the build. The crate `compile_error!`s
//! when no language feature is enabled — we have no need for a registry that
//! supports zero languages, and excluding the variant guard keeps the public
//! API total.

use std::path::Path;
use std::sync::OnceLock;

use tree_sitter::{Language as TsLanguage, Query};
#[cfg(feature = "lang-ts")]
use tree_sitter_typescript::{LANGUAGE_TSX, LANGUAGE_TYPESCRIPT};

#[cfg(not(any(feature = "lang-ts", feature = "lang-rust")))]
compile_error!("heal-observer requires at least one language feature: lang-ts or lang-rust");

#[cfg(feature = "lang-ts")]
const TYPESCRIPT_FUNCTIONS_QUERY: &str = include_str!("../../queries/typescript/functions.scm");
#[cfg(feature = "lang-ts")]
const TYPESCRIPT_CCN_QUERY: &str = include_str!("../../queries/typescript/ccn.scm");
#[cfg(feature = "lang-ts")]
const TYPESCRIPT_COGNITIVE_QUERY: &str = include_str!("../../queries/typescript/cognitive.scm");
#[cfg(feature = "lang-ts")]
const TYPESCRIPT_LCOM_QUERY: &str = include_str!("../../queries/typescript/lcom.scm");

#[cfg(feature = "lang-rust")]
const RUST_FUNCTIONS_QUERY: &str = include_str!("../../queries/rust/functions.scm");
#[cfg(feature = "lang-rust")]
const RUST_CCN_QUERY: &str = include_str!("../../queries/rust/ccn.scm");
#[cfg(feature = "lang-rust")]
const RUST_COGNITIVE_QUERY: &str = include_str!("../../queries/rust/cognitive.scm");
#[cfg(feature = "lang-rust")]
const RUST_LCOM_QUERY: &str = include_str!("../../queries/rust/lcom.scm");

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    #[cfg(feature = "lang-ts")]
    TypeScript,
    #[cfg(feature = "lang-ts")]
    Tsx,
    #[cfg(feature = "lang-rust")]
    Rust,
}

/// A compiled tree-sitter `Query` paired with its typed capture indices.
/// The `captures` payload differs per query role (functions/CCN/cognitive).
pub struct CompiledQuery<C: 'static> {
    pub query: Query,
    pub captures: C,
}

pub struct FunctionCaptures {
    pub scope: u32,
}

pub struct CcnCaptures {
    pub point: u32,
    pub binary: u32,
}

pub struct CognitiveCaptures {
    pub if_: u32,
    pub else_: u32,
    pub inc_and_nest: u32,
    pub inc: u32,
    pub binary: u32,
}

pub struct LcomCaptures {
    pub class_scope: u32,
}

struct LanguageQueries {
    functions: OnceLock<CompiledQuery<FunctionCaptures>>,
    ccn: OnceLock<CompiledQuery<CcnCaptures>>,
    cognitive: OnceLock<CompiledQuery<CognitiveCaptures>>,
    lcom: OnceLock<CompiledQuery<LcomCaptures>>,
}

impl LanguageQueries {
    const fn new() -> Self {
        Self {
            functions: OnceLock::new(),
            ccn: OnceLock::new(),
            cognitive: OnceLock::new(),
            lcom: OnceLock::new(),
        }
    }
}

#[cfg(feature = "lang-ts")]
static TYPESCRIPT_QUERIES: LanguageQueries = LanguageQueries::new();
#[cfg(feature = "lang-ts")]
static TSX_QUERIES: LanguageQueries = LanguageQueries::new();
#[cfg(feature = "lang-rust")]
static RUST_QUERIES: LanguageQueries = LanguageQueries::new();

impl Language {
    /// Dispatch on file extension. Returns `None` for unsupported types — the
    /// caller decides whether that's a skip or an error.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        match ext {
            #[cfg(feature = "lang-ts")]
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            #[cfg(feature = "lang-ts")]
            "tsx" => Some(Self::Tsx),
            #[cfg(feature = "lang-rust")]
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }

    /// Display name (stable; used in serialized output).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            #[cfg(feature = "lang-ts")]
            Self::TypeScript => "typescript",
            #[cfg(feature = "lang-ts")]
            Self::Tsx => "tsx",
            #[cfg(feature = "lang-rust")]
            Self::Rust => "rust",
        }
    }

    #[must_use]
    pub fn ts_language(self) -> TsLanguage {
        match self {
            #[cfg(feature = "lang-ts")]
            Self::TypeScript => LANGUAGE_TYPESCRIPT.into(),
            #[cfg(feature = "lang-ts")]
            Self::Tsx => LANGUAGE_TSX.into(),
            #[cfg(feature = "lang-rust")]
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }

    #[must_use]
    pub fn functions_query(self) -> &'static CompiledQuery<FunctionCaptures> {
        self.cache().functions.get_or_init(|| {
            let lang = self.ts_language();
            let query = Query::new(&lang, self.functions_query_source())
                .expect("functions.scm must compile");
            let captures = FunctionCaptures {
                scope: capture_index(&query, "function.scope"),
            };
            CompiledQuery { query, captures }
        })
    }

    #[must_use]
    pub fn ccn_query(self) -> &'static CompiledQuery<CcnCaptures> {
        self.cache().ccn.get_or_init(|| {
            let lang = self.ts_language();
            let query = Query::new(&lang, self.ccn_query_source()).expect("ccn.scm must compile");
            let captures = CcnCaptures {
                point: capture_index(&query, "ccn.point"),
                binary: capture_index(&query, "ccn.binary"),
            };
            CompiledQuery { query, captures }
        })
    }

    #[must_use]
    pub fn cognitive_query(self) -> &'static CompiledQuery<CognitiveCaptures> {
        self.cache().cognitive.get_or_init(|| {
            let lang = self.ts_language();
            let query = Query::new(&lang, self.cognitive_query_source())
                .expect("cognitive.scm must compile");
            let captures = CognitiveCaptures {
                if_: capture_index(&query, "if"),
                else_: capture_index(&query, "else"),
                inc_and_nest: capture_index(&query, "inc_and_nest"),
                inc: capture_index(&query, "inc"),
                binary: capture_index(&query, "binary"),
            };
            CompiledQuery { query, captures }
        })
    }

    #[must_use]
    pub fn lcom_query(self) -> &'static CompiledQuery<LcomCaptures> {
        self.cache().lcom.get_or_init(|| {
            let lang = self.ts_language();
            let query = Query::new(&lang, self.lcom_query_source()).expect("lcom.scm must compile");
            let captures = LcomCaptures {
                class_scope: capture_index(&query, "class.scope"),
            };
            CompiledQuery { query, captures }
        })
    }

    fn cache(self) -> &'static LanguageQueries {
        match self {
            #[cfg(feature = "lang-ts")]
            Self::TypeScript => &TYPESCRIPT_QUERIES,
            #[cfg(feature = "lang-ts")]
            Self::Tsx => &TSX_QUERIES,
            #[cfg(feature = "lang-rust")]
            Self::Rust => &RUST_QUERIES,
        }
    }

    fn functions_query_source(self) -> &'static str {
        match self {
            #[cfg(feature = "lang-ts")]
            Self::TypeScript | Self::Tsx => TYPESCRIPT_FUNCTIONS_QUERY,
            #[cfg(feature = "lang-rust")]
            Self::Rust => RUST_FUNCTIONS_QUERY,
        }
    }

    fn ccn_query_source(self) -> &'static str {
        match self {
            #[cfg(feature = "lang-ts")]
            Self::TypeScript | Self::Tsx => TYPESCRIPT_CCN_QUERY,
            #[cfg(feature = "lang-rust")]
            Self::Rust => RUST_CCN_QUERY,
        }
    }

    fn cognitive_query_source(self) -> &'static str {
        match self {
            #[cfg(feature = "lang-ts")]
            Self::TypeScript | Self::Tsx => TYPESCRIPT_COGNITIVE_QUERY,
            #[cfg(feature = "lang-rust")]
            Self::Rust => RUST_COGNITIVE_QUERY,
        }
    }

    fn lcom_query_source(self) -> &'static str {
        match self {
            #[cfg(feature = "lang-ts")]
            Self::TypeScript | Self::Tsx => TYPESCRIPT_LCOM_QUERY,
            #[cfg(feature = "lang-rust")]
            Self::Rust => RUST_LCOM_QUERY,
        }
    }
}

fn capture_index(query: &Query, name: &str) -> u32 {
    query
        .capture_index_for_name(name)
        .unwrap_or_else(|| panic!("query missing @{name} capture"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[cfg(feature = "lang-ts")]
    #[test]
    fn dispatches_typescript_extensions() {
        assert_eq!(
            Language::from_path(&PathBuf::from("foo.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::from_path(&PathBuf::from("nested/dir/foo.mts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::from_path(&PathBuf::from("foo.cts")),
            Some(Language::TypeScript)
        );
    }

    #[cfg(feature = "lang-ts")]
    #[test]
    fn dispatches_tsx_extension() {
        assert_eq!(
            Language::from_path(&PathBuf::from("Component.tsx")),
            Some(Language::Tsx)
        );
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn dispatches_rust_extension() {
        assert_eq!(
            Language::from_path(&PathBuf::from("crates/core/src/lib.rs")),
            Some(Language::Rust)
        );
    }

    #[test]
    fn rejects_unsupported_extensions() {
        // JavaScript / Python / docs / config types are not part of v0.1's
        // grammar set even with all language features enabled.
        assert_eq!(Language::from_path(&PathBuf::from("foo.js")), None);
        assert_eq!(Language::from_path(&PathBuf::from("foo.jsx")), None);
        assert_eq!(Language::from_path(&PathBuf::from("foo.py")), None);
        assert_eq!(Language::from_path(&PathBuf::from("README.md")), None);
        assert_eq!(Language::from_path(&PathBuf::from("Cargo.toml")), None);
    }

    #[test]
    fn rejects_extensionless_paths() {
        assert_eq!(Language::from_path(&PathBuf::from("Makefile")), None);
        assert_eq!(Language::from_path(&PathBuf::from("")), None);
    }

    #[cfg(all(feature = "lang-ts", feature = "lang-rust"))]
    #[test]
    fn loads_grammars() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&Language::TypeScript.ts_language())
            .expect("typescript grammar loads");
        parser
            .set_language(&Language::Tsx.ts_language())
            .expect("tsx grammar loads");
        parser
            .set_language(&Language::Rust.ts_language())
            .expect("rust grammar loads");
    }

    #[cfg(all(feature = "lang-ts", feature = "lang-rust"))]
    #[test]
    fn cached_queries_compile_and_index() {
        for lang in [Language::TypeScript, Language::Tsx, Language::Rust] {
            let f = lang.functions_query();
            assert!(f.query.pattern_count() > 0);
            let _ = f.captures.scope;

            let c = lang.ccn_query();
            assert!(c.query.pattern_count() > 0);
            let _ = (c.captures.point, c.captures.binary);

            let g = lang.cognitive_query();
            assert!(g.query.pattern_count() > 0);
            let _ = (
                g.captures.if_,
                g.captures.else_,
                g.captures.inc_and_nest,
                g.captures.inc,
                g.captures.binary,
            );
        }
    }
}
