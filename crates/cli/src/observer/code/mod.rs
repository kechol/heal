//! Code-feature observer family. Reads project source + git history and
//! emits the v0.x code-substrate Findings. Per-metric on/off lives under
//! `[metrics.<m>]` in `.heal/config.toml`.

pub mod change_coupling;
pub mod churn;
pub mod complexity;
pub mod duplication;
pub mod hotspot;
pub mod lcom;
pub mod loc;

use std::path::Path;

use crate::observer::shared::lang::Language;
use crate::observer::shared::walk::{walk_supported_files_under, ExcludeMatcher};

/// Walk the source tree once, parse each supported file once, and feed
/// every requested accumulator. Complexity, Duplication, and LCOM all
/// walk the same file set (same `cfg.exclude_lines()`, same workspace
/// scope in the orchestrator), so scanning them together removes an
/// up-to-3× read + tree-sitter-parse cost per run. Each observer's
/// `scan` delegates here with only its own accumulator, so standalone
/// behavior is unchanged.
pub(crate) fn scan_source_tree(
    root: &Path,
    excluded: &[String],
    workspace: Option<&Path>,
    mut complexity: Option<&mut complexity::ComplexityAccumulator>,
    mut duplication: Option<&mut duplication::DuplicationAccumulator>,
    mut lcom: Option<&mut lcom::LcomAccumulator>,
) {
    if complexity.is_none() && duplication.is_none() && lcom.is_none() {
        return;
    }
    let matcher =
        ExcludeMatcher::compile(root, excluded).expect("exclude patterns validated at config load");
    for path in walk_supported_files_under(root, &matcher, workspace) {
        let lang = Language::from_path(&path).expect("walker filters by Language::from_path");
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = complexity::parse(source, lang) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .map_or_else(|_| path.clone(), Path::to_path_buf);
        if let Some(acc) = complexity.as_deref_mut() {
            acc.add(&rel, lang, &parsed);
        }
        if let Some(acc) = duplication.as_deref_mut() {
            acc.add(&rel, &parsed);
        }
        if let Some(acc) = lcom.as_deref_mut() {
            acc.add(&rel, lang, &parsed);
        }
    }
}
