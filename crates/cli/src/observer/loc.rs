//! LOC (lines of code) and language detection via the `tokei` crate.
//!
//! `LocObserver` is a thin wrapper around `tokei::Languages::get_statistics`.
//! It is intentionally **not** gated by a `MetricsConfig` toggle: LOC is a
//! foundational signal that other observers (hotspot, churn weighting) and
//! `heal init` (primary-language auto-detect) depend on.
//!
//! Primary-language selection ignores `LanguageType::is_literate` (Markdown,
//! Org, etc.) so a docs-heavy repo still picks up its actual implementation
//! language. `exclude_languages` further removes language entries entirely.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tokei::{Config as TokeiConfig, LanguageType, Languages};

use crate::core::config::Config;

use crate::observer::{ObservationMeta, Observer};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineCounts {
    pub code: usize,
    pub comments: usize,
    pub blanks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageStats {
    /// Stable English name from `tokei::LanguageType::name` (e.g. "TypeScript").
    pub name: String,
    pub files: usize,
    #[serde(flatten)]
    pub counts: LineCounts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocReport {
    /// Languages sorted by `code` lines descending.
    pub languages: Vec<LanguageStats>,
    /// Highest-`code` non-literate language. `None` for empty trees or trees
    /// containing only literate sources.
    pub primary: Option<String>,
    /// Sum across all retained languages (after exclusions).
    pub totals: LineCounts,
}

impl LocReport {
    #[must_use]
    pub fn total_files(&self) -> usize {
        self.languages.iter().map(|e| e.files).sum()
    }
}

/// Thin wrapper around `tokei::Languages::get_statistics`. Stateless;
/// constructing one is cheap.
#[derive(Debug, Clone, Default)]
pub struct LocObserver {
    /// Substrings checked against every visited path; matches are skipped.
    /// Gitignore-style exclude lines applied to the post-tokei file
    /// list (tokei's own `excluded` argument has substring semantics
    /// that don't match HEAL's gitignore contract; we filter
    /// `lang.reports` ourselves and re-aggregate).
    pub excluded: Vec<String>,
    /// Language names (matching `LanguageType::name`) to drop from the report
    /// entirely. Useful for excluding lockfiles, JSON dumps, etc.
    pub exclude_languages: Vec<String>,
}

impl LocObserver {
    /// Build a `LocObserver` from a loaded `heal-core` `Config`.
    ///
    /// `git.exclude_paths` is folded in iff `metrics.loc.inherit_git_excludes`
    /// is true (default), then `metrics.loc.exclude_paths` is appended.
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            excluded: cfg.exclude_lines(),
            exclude_languages: Vec::new(),
        }
    }

    /// Run the scan and produce a `LocReport`. Pure function over `root`.
    pub fn scan(&self, root: &Path) -> LocReport {
        let mut languages = Languages::new();
        let paths = [root];
        // Pass an empty exclude list to tokei — its `&[&str]` substring
        // semantics don't match HEAL's gitignore DSL. Filter the
        // per-file `reports` ourselves below and re-aggregate.
        languages.get_statistics(&paths, &[], &TokeiConfig::default());
        let matcher = crate::observer::walk::ExcludeMatcher::compile(root, &self.excluded)
            .expect("exclude patterns validated at config load");

        let mut entries = Vec::with_capacity(languages.len());
        let mut totals = LineCounts::default();
        for (lang_type, lang) in &languages {
            let name = lang_type.name().to_string();
            if self
                .exclude_languages
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            // Re-aggregate from the surviving per-file reports so the
            // language totals reflect post-exclude counts. tokei keeps
            // `lang.code` etc. as the unfiltered sum, which would
            // overstate after a `vendor/` exclusion drops half the
            // files.
            let mut code = 0usize;
            let mut comments = 0usize;
            let mut blanks = 0usize;
            let mut files = 0usize;
            for report in &lang.reports {
                if matcher.is_excluded(&report.name, false) {
                    continue;
                }
                code += report.stats.code;
                comments += report.stats.comments;
                blanks += report.stats.blanks;
                files += 1;
            }
            if files == 0 {
                continue;
            }
            totals.code += code;
            totals.comments += comments;
            totals.blanks += blanks;
            entries.push(LanguageStats {
                name,
                files,
                counts: LineCounts {
                    code,
                    comments,
                    blanks,
                },
            });
        }

        entries.sort_by(|a, b| b.counts.code.cmp(&a.counts.code).then(a.name.cmp(&b.name)));

        let primary = entries
            .iter()
            .find(|e| !is_literate_name(&e.name))
            .map(|e| e.name.clone());

        LocReport {
            languages: entries,
            primary,
            totals,
        }
    }
}

/// Resolve a tokei-emitted display name back to its `LanguageType` and
/// consult `is_literate`. Markdown / Org / etc. are filtered for the
/// primary-language choice but kept in `languages` for visibility.
fn is_literate_name(name: &str) -> bool {
    LanguageType::from_name(name).is_some_and(LanguageType::is_literate)
}

impl Observer for LocObserver {
    type Output = LocReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "loc",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}
