//! Cross-file duplication detection via Rabin-Karp rolling hashes over
//! tree-sitter token streams.
//!
//! Each supported source file is parsed, leaf nodes (excluding tree-sitter
//! "extras" like comments/whitespace and parse errors) are extracted as a
//! token stream where every token is reduced to a 64-bit identity hash of
//! `(kind_id, text)` — pure type-1 (exact) clone detection. We then slide a
//! window of `min_tokens` over each file, compute a Rabin-Karp polynomial
//! rolling hash, and bucket windows by hash. Buckets with ≥2 entries are
//! verified by comparing the underlying hash slices (collision check
//! against the per-token identity hash, which has astronomically low
//! collision probability for a sequence of tens of tokens) and then greedy-
//! extended forward as far as every location agrees, yielding a single
//! maximal block instead of N − `min_tokens` + 1 overlapping minimal blocks.
//!
//! Limitations explicitly out of v0.1 scope:
//! - Type-2 clones (identifier-insensitive). Token hash includes text, so
//!   `function foo` and `function bar` won't match.
//! - Parallel scanning. The walker is single-threaded.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tree_sitter::TreeCursor;

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};

use crate::observer::complexity::{parse, ParsedFile};
use crate::observer::lang::Language;
use crate::observer::walk::walk_supported_files_under;
use crate::observer::{ObservationMeta, Observer};
use crate::observers::ObserverReports;

/// FNV-1a 64-bit prime — used both as the per-token identity hash multiplier
/// and as the polynomial base for the Rabin-Karp window hash. Mixed via
/// `wrapping_*` arithmetic; collisions in the rolling hash are caught by
/// the post-bucket exact slice comparison.
const HASH_BASE: u64 = 0x100_0000_01b3;
/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

#[derive(Debug, Clone, Default)]
pub struct DuplicationObserver {
    pub enabled: bool,
    pub excluded: Vec<String>,
    /// Window size in tokens. Below this size matches are dropped to
    /// suppress incidental repetition (imports, type annotations, etc.).
    pub min_tokens: u32,
    /// Optional workspace sub-path; see `ComplexityObserver::workspace`.
    pub workspace: Option<PathBuf>,
}

impl DuplicationObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.metrics.duplication.enabled,
            excluded: cfg.observer_excluded_paths(),
            min_tokens: cfg.metrics.duplication.min_tokens,
            workspace: None,
        }
    }

    #[must_use]
    pub fn with_workspace(mut self, workspace: Option<PathBuf>) -> Self {
        self.workspace = workspace;
        self
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> DuplicationReport {
        let mut report = DuplicationReport {
            min_tokens: self.min_tokens,
            ..DuplicationReport::default()
        };
        if !self.enabled || self.min_tokens == 0 {
            return report;
        }
        let window = self.min_tokens as usize;
        let mut files: Vec<FileTokens> = Vec::new();
        for path in walk_supported_files_under(root, &self.excluded, self.workspace.as_deref()) {
            let Some(lang) = Language::from_path(&path) else {
                continue;
            };
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(parsed) = parse(source, lang) else {
                continue;
            };
            let (hashes, lines) = collect_tokens(&parsed);
            if hashes.len() < window {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .unwrap_or(path);
            files.push(FileTokens {
                path: rel,
                hashes,
                lines,
            });
        }

        if files.is_empty() {
            return report;
        }

        let report_blocks = assemble_report_blocks(detect_blocks(&files, window), &files);
        let (files_summary, affected_count, duplicate_tokens) =
            file_level_rollup(&files, &report_blocks);

        report.totals = DuplicationTotals {
            duplicate_blocks: report_blocks.len(),
            duplicate_tokens,
            files_affected: affected_count,
        };
        report.blocks = report_blocks;
        report.files = files_summary;
        report
    }
}

/// Lift `detect_blocks`'s `(file_index, start_token)` locations into
/// the public `DuplicateBlock` shape and sort everything for stable
/// output: each block's locations canonicalise by `(path, start_line)`,
/// and the block list itself sorts by `token_count` desc then by the
/// first location's path/line.
fn assemble_report_blocks(blocks: Vec<InternalBlock>, files: &[FileTokens]) -> Vec<DuplicateBlock> {
    let mut report_blocks: Vec<DuplicateBlock> = blocks
        .into_iter()
        .map(|b| {
            let locations = b
                .locations
                .into_iter()
                .map(|(fi, start)| {
                    let f = &files[fi];
                    let start_line = f.lines.get(start).copied().unwrap_or(0);
                    let end_line = f
                        .lines
                        .get(start + b.token_count - 1)
                        .copied()
                        .unwrap_or(start_line);
                    DuplicateLocation {
                        path: f.path.clone(),
                        start_line,
                        end_line,
                    }
                })
                .collect::<Vec<_>>();
            let mut block = DuplicateBlock {
                token_count: u32::try_from(b.token_count).unwrap_or(u32::MAX),
                locations,
            };
            block.locations.sort_by(|x, y| {
                x.path
                    .cmp(&y.path)
                    .then_with(|| x.start_line.cmp(&y.start_line))
            });
            block
        })
        .collect();
    report_blocks.sort_by(|a, b| {
        b.token_count
            .cmp(&a.token_count)
            .then_with(|| a.locations[0].path.cmp(&b.locations[0].path))
            .then_with(|| a.locations[0].start_line.cmp(&b.locations[0].start_line))
    });
    report_blocks
}

/// Roll block-level locations up to a per-file summary plus the
/// global `(affected_count, duplicate_tokens)` totals. Every scanned
/// file appears in the summary (zero-dup included) so calibration
/// distributions see the full sample.
fn file_level_rollup(
    files: &[FileTokens],
    blocks: &[DuplicateBlock],
) -> (Vec<FileDuplication>, usize, usize) {
    let mut dup_tokens_by_file: std::collections::BTreeMap<PathBuf, u32> =
        std::collections::BTreeMap::new();
    let mut duplicate_tokens: usize = 0;
    for b in blocks {
        duplicate_tokens += b.token_count as usize * b.locations.len();
        for loc in &b.locations {
            let entry = dup_tokens_by_file.entry(loc.path.clone()).or_insert(0);
            *entry = entry.saturating_add(b.token_count);
        }
    }

    let mut summary: Vec<FileDuplication> = files
        .iter()
        .map(|f| {
            let total_tokens = u32::try_from(f.hashes.len()).unwrap_or(u32::MAX);
            let dt = dup_tokens_by_file
                .get(&f.path)
                .copied()
                .unwrap_or(0)
                .min(total_tokens);
            let pct = if total_tokens == 0 {
                0.0
            } else {
                f64::from(dt) / f64::from(total_tokens) * 100.0
            };
            FileDuplication {
                path: f.path.clone(),
                total_tokens,
                duplicate_tokens: dt,
                duplicate_pct: pct,
            }
        })
        .collect();
    summary.sort_by(|a, b| a.path.cmp(&b.path));

    (summary, dup_tokens_by_file.len(), duplicate_tokens)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DuplicationReport {
    pub blocks: Vec<DuplicateBlock>,
    /// Per-file roll-up. One entry per scanned file (even those with
    /// zero duplication), sorted by path. `duplicate_pct` is the
    /// calibration input — see TODO §「v0.2 範囲のメトリクス対象 /
    /// Duplication % / 30%」.
    #[serde(default)]
    pub files: Vec<FileDuplication>,
    pub totals: DuplicationTotals,
    pub min_tokens: u32,
}

impl Eq for DuplicationReport {}

impl DuplicationReport {
    /// Top-N duplicate blocks by token count (descending). The underlying
    /// `blocks` vector is already sorted at scan time.
    #[must_use]
    pub fn worst_n_blocks(&self, n: usize) -> Vec<DuplicateBlock> {
        let mut top = self.blocks.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicationTotals {
    pub duplicate_blocks: usize,
    pub duplicate_tokens: usize,
    pub files_affected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateBlock {
    pub token_count: u32,
    pub locations: Vec<DuplicateLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicateLocation {
    pub path: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileDuplication {
    pub path: PathBuf,
    pub total_tokens: u32,
    pub duplicate_tokens: u32,
    pub duplicate_pct: f64,
}

impl Eq for FileDuplication {}

impl Observer for DuplicationObserver {
    type Output = DuplicationReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "duplication",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}

impl IntoFindings for DuplicationReport {
    /// The first location (already canonical-sorted by `(path,
    /// start_line)` at scan time) becomes `location`; the rest land in
    /// `locations`. The content seed mixes every site's
    /// `path:start_line` plus `token_count`, so the same block
    /// re-detected on a later commit reuses its id.
    fn into_findings(&self) -> Vec<Finding> {
        self.blocks
            .iter()
            .map(|block| {
                let (head, rest) = block
                    .locations
                    .split_first()
                    .expect("DuplicateBlock always has ≥2 locations");
                let primary = Location {
                    file: head.path.clone(),
                    line: Some(head.start_line),
                    symbol: None,
                };
                let mut seed = format!("dup:{}:", block.token_count);
                let _ = write!(seed, "{}:{};", head.path.to_string_lossy(), head.start_line);
                let mut extras: Vec<Location> = Vec::with_capacity(rest.len());
                for l in rest {
                    let _ = write!(seed, "{}:{};", l.path.to_string_lossy(), l.start_line);
                    extras.push(Location {
                        file: l.path.clone(),
                        line: Some(l.start_line),
                        symbol: None,
                    });
                }
                let summary = format!(
                    "{} tokens duplicated across {} sites",
                    block.token_count,
                    1 + extras.len()
                );
                Finding::new("duplication", primary, summary, &seed).with_locations(extras)
            })
            .collect()
    }
}

struct FileTokens {
    path: PathBuf,
    hashes: Vec<u64>,
    lines: Vec<u32>,
}

struct InternalBlock {
    token_count: usize,
    locations: Vec<(usize, usize)>,
}

/// Walk the parsed tree pre-order and collect every leaf token that isn't
/// an `extra` (tree-sitter convention for comments / whitespace) or an
/// error fragment. Returns parallel hash + line vectors.
fn collect_tokens(parsed: &ParsedFile) -> (Vec<u64>, Vec<u32>) {
    let mut hashes: Vec<u64> = Vec::new();
    let mut lines: Vec<u32> = Vec::new();
    let source = parsed.source.as_bytes();
    let mut cursor: TreeCursor<'_> = parsed.tree.walk();
    loop {
        let node = cursor.node();
        if let Some((hash, line)) = leaf_token(&node, source) {
            hashes.push(hash);
            lines.push(line);
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return (hashes, lines);
            }
        }
    }
}

/// Returns the token-identity hash + 1-based start line for a real leaf
/// token, or `None` for branches / extras / errors / whitespace-only text.
///
/// Uses a hand-rolled FNV-1a 64-bit hash so the per-token identity is
/// reproducible across processes and Rust toolchains — `std::hash::DefaultHasher`
/// is explicitly *not* stable across releases.
fn leaf_token(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<(u64, u32)> {
    if node.child_count() != 0 || node.is_extra() || node.is_error() || node.is_missing() {
        return None;
    }
    let text = node.utf8_text(source).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    let mut h = FNV_OFFSET;
    for b in node.kind_id().to_le_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(HASH_BASE);
    }
    for b in text.as_bytes() {
        h = (h ^ u64::from(*b)).wrapping_mul(HASH_BASE);
    }
    let line = u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX);
    Some((h, line))
}

/// Build the per-window Rabin-Karp hash list for a file's token stream.
/// Returns an empty vec if the file is shorter than `window`.
fn compute_window_hashes(tokens: &[u64], window: usize) -> Vec<u64> {
    if tokens.len() < window || window == 0 {
        return Vec::new();
    }
    let exp = u32::try_from(window).unwrap_or(u32::MAX).saturating_sub(1);
    let base_pow = HASH_BASE.wrapping_pow(exp);

    let mut out = Vec::with_capacity(tokens.len() - window + 1);
    let mut h: u64 = 0;
    for &t in &tokens[..window] {
        h = h.wrapping_mul(HASH_BASE).wrapping_add(t);
    }
    out.push(h);
    for k in window..tokens.len() {
        let oldest = tokens[k - window];
        h = h
            .wrapping_sub(oldest.wrapping_mul(base_pow))
            .wrapping_mul(HASH_BASE)
            .wrapping_add(tokens[k]);
        out.push(h);
    }
    out
}

fn detect_blocks(files: &[FileTokens], window: usize) -> Vec<InternalBlock> {
    if window == 0 {
        return Vec::new();
    }

    // Per-file rolling hashes: `window_hashes[fi][start]` is the hash of the
    // length-`window` slice beginning at index `start`. Computed once and
    // reused both for bucketing and verification, so each window's hash is
    // calculated exactly once.
    let window_hashes: Vec<Vec<u64>> = files
        .iter()
        .map(|f| compute_window_hashes(&f.hashes, window))
        .collect();

    let mut buckets: HashMap<u64, Vec<(usize, usize)>> = HashMap::new();
    for (fi, hashes) in window_hashes.iter().enumerate() {
        for (start, &h) in hashes.iter().enumerate() {
            buckets.entry(h).or_default().push((fi, start));
        }
    }

    let mut covered: Vec<Vec<bool>> = files
        .iter()
        .map(|f| vec![false; f.hashes.len().max(1)])
        .collect();
    let mut blocks: Vec<InternalBlock> = Vec::new();

    for fi in 0..files.len() {
        let n = files[fi].hashes.len();
        if n < window {
            continue;
        }
        let last_start = n - window;
        for start in 0..=last_start {
            if covered[fi][start] {
                continue;
            }
            let h = window_hashes[fi][start];
            let Some(bucket) = buckets.get(&h) else {
                continue;
            };
            if bucket.len() < 2 {
                continue;
            }

            // Verify by comparing the per-token hash slices and dropping any
            // location that's already been covered by an earlier block.
            let base_window = &files[fi].hashes[start..start + window];
            let mut locs: Vec<(usize, usize)> = bucket
                .iter()
                .copied()
                .filter(|(bf, bs)| {
                    if covered[*bf][*bs] {
                        return false;
                    }
                    let other = &files[*bf];
                    other.hashes.len() >= bs + window
                        && &other.hashes[*bs..*bs + window] == base_window
                })
                .collect();
            // dedupe (in case a window appears multiple times in the same
            // file at the same start — shouldn't happen, but defensive)
            locs.sort_unstable();
            locs.dedup();
            if locs.len() < 2 {
                continue;
            }

            // Greedy-extend forward as long as every location agrees.
            let mut len = window;
            loop {
                let next = len;
                let (rf, rs) = locs[0];
                let Some(reference) = files[rf].hashes.get(rs + next).copied() else {
                    break;
                };
                let mut all_match = true;
                for &(lf, ls) in locs.iter().skip(1) {
                    if files[lf].hashes.get(ls + next).copied() != Some(reference) {
                        all_match = false;
                        break;
                    }
                }
                if !all_match {
                    break;
                }
                len += 1;
            }

            // Mark every contained window-start as covered for each location.
            let cover_count = len - window + 1;
            for &(lf, ls) in &locs {
                for c in ls..(ls + cover_count) {
                    if let Some(slot) = covered[lf].get_mut(c) {
                        *slot = true;
                    }
                }
            }

            blocks.push(InternalBlock {
                token_count: len,
                locations: locs,
            });
        }
    }

    blocks
}

pub struct DuplicationFeature;

impl Feature for DuplicationFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "duplication",
            version: 1,
            kind: FeatureKind::Observer,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.metrics.duplication.enabled
    }
    fn lower(
        &self,
        reports: &ObserverReports,
        cfg: &Config,
        cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(dup) = reports.duplication.as_ref() else {
            return Vec::new();
        };
        let workspaces = cfg.project.workspaces.as_slice();
        let pct_by_file: HashMap<&Path, f64> = dup
            .files
            .iter()
            .map(|f| (f.path.as_path(), f.duplicate_pct))
            .collect();
        let mut out = Vec::with_capacity(dup.blocks.len());
        for (block, finding) in dup.blocks.iter().zip(dup.into_findings()) {
            let max_pct = block
                .locations
                .iter()
                .filter_map(|l| pct_by_file.get(l.path.as_path()).copied())
                .fold(0.0_f64, f64::max);
            // Calibration follows the finding's primary location — the
            // block's first site, which is what carries the `Finding.id`.
            let cal_dup = cal
                .metrics_for_file(&finding.location.file, workspaces)
                .duplication
                .as_ref();
            let severity = cal_dup.map_or(Severity::Ok, |c| c.classify(max_pct));
            out.push(decorate(finding, severity, hotspot));
        }
        out
    }
}
