//! Co-change analysis: which files tend to be modified together?
//!
//! For every reachable commit inside the `since_days` window, we extract
//! the set of paths it touches (first-parent diff to avoid merge-driven
//! double-counting) and increment a counter for every unordered pair in
//! that set. The pair counters become the "coupling" signal — files that
//! consistently change together expose hidden behavioural coupling that
//! static analysis can't see (KNOWLEDGE.md § 3.5).
//!
//! ## Symmetric vs one-way pairs
//!
//! Per-pair, we also remember each file's individual commit count
//! (`file_commits`). This lets us split the surviving pairs into:
//!
//! - **Symmetric** — both files rarely change without the other.
//!   `min(P(B|A), P(A|B)) >= symmetric_threshold`. The strongest "mixed
//!   responsibility" signal: the pair behaves as one unit even though
//!   the filesystem says they're two files.
//! - **`OneWay { from, to }`** — `from` often changes alone; `to` almost
//!   always shows up alongside `from`. Changes flow `from → to`. The
//!   leader is the file with the higher conditional probability of
//!   *being co-changed* (i.e. the file whose changes the other depends
//!   on).
//!
//! Both variants flow into the same Calibration entry
//! (`cal.change_coupling`), but the Finding metric tag differs
//! (`change_coupling.symmetric` vs `change_coupling`) so the renderer
//! can call out the symmetric case separately.
//!
//! ## Bulk commit cap
//!
//! Bulk commits (lockfile bumps, mass renames, generated-code refreshes)
//! would otherwise dominate the pair-count quadratic blow-up. We hard-cap
//! the per-commit fan-out at `BULK_COMMIT_FILE_LIMIT`; configurable knob
//! is deferred to v0.2.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use git2::{Repository, Sort};
use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};

use crate::observer::walk::{is_path_excluded, since_cutoff};
use crate::observer::{ObservationMeta, Observer};
use crate::observers::ObserverReports;

/// Skip commits whose changed-file count exceeds this limit. The pair count
/// grows O(N²) per commit so bulk merges (think 200-file lockfile bumps)
/// would otherwise drown the signal. v0.2 may expose this as TOML config.
const BULK_COMMIT_FILE_LIMIT: usize = 50;

#[derive(Debug, Clone)]
pub struct ChangeCouplingObserver {
    pub enabled: bool,
    pub excluded: Vec<String>,
    pub since_days: u32,
    /// Pairs with fewer than `min_coupling` co-occurrences are dropped from
    /// the report. Sourced from `metrics.change_coupling.min_coupling`.
    pub min_coupling: u32,
    /// Pairs whose co-occurrence is below `min_lift × chance` are
    /// dropped. See [`ChangeCouplingConfig::min_lift`]. Default 2.0.
    pub min_lift: f64,
    /// Threshold both `P(B|A)` and `P(A|B)` must meet for a pair to
    /// classify as `Symmetric`. Default 0.5 — at least half of each
    /// file's edits must coincide with the partner. Below it, the pair
    /// is a `OneWay` flow.
    pub symmetric_threshold: f64,
}

impl Default for ChangeCouplingObserver {
    fn default() -> Self {
        Self {
            enabled: false,
            excluded: Vec::new(),
            since_days: 0,
            min_coupling: 0,
            min_lift: 0.0,
            symmetric_threshold: default_symmetric_threshold(),
        }
    }
}

impl ChangeCouplingObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.metrics.change_coupling.enabled,
            excluded: cfg.observer_excluded_paths(),
            since_days: cfg.git.since_days,
            min_coupling: cfg.metrics.change_coupling.min_coupling,
            min_lift: cfg.metrics.change_coupling.min_lift,
            symmetric_threshold: cfg.metrics.change_coupling.symmetric_threshold,
        }
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> ChangeCouplingReport {
        let mut report = ChangeCouplingReport {
            since_days: self.since_days,
            min_coupling: self.min_coupling,
            ..ChangeCouplingReport::default()
        };
        if !self.enabled {
            return report;
        }
        let Ok(repo) = Repository::discover(root) else {
            return report;
        };
        let cutoff_secs = since_cutoff(self.since_days);
        let Ok(mut revwalk) = repo.revwalk() else {
            return report;
        };
        if revwalk.set_sorting(Sort::TIME).is_err() || revwalk.push_head().is_err() {
            return report;
        }

        let mut pair_counts: HashMap<(PathBuf, PathBuf), u32> = HashMap::new();
        let mut file_commits: HashMap<PathBuf, u32> = HashMap::new();
        let mut commits_considered: u32 = 0;

        for oid_res in revwalk {
            let Ok(oid) = oid_res else {
                continue;
            };
            let Ok(commit) = repo.find_commit(oid) else {
                continue;
            };
            if commit.time().seconds() < cutoff_secs {
                break;
            }
            if self.absorb_commit(&repo, &commit, &mut pair_counts, &mut file_commits) {
                commits_considered = commits_considered.saturating_add(1);
            }
        }

        let pairs = collect_pairs(
            pair_counts,
            self.min_coupling,
            self.min_lift,
            &file_commits,
            commits_considered,
            self.symmetric_threshold,
        );
        let file_sums = compute_file_sums(&pairs);

        let totals = CouplingTotals {
            pairs: pairs.len(),
            files: file_sums.len(),
            commits_considered,
        };
        report.pairs = pairs;
        report.file_sums = file_sums;
        report.totals = totals;
        report
    }

    /// Returns `true` if the commit's filtered changeset contributed pair
    /// counts (i.e. landed within the bulk-commit limit and had ≥2 files).
    /// Also bumps every surviving file's individual commit counter
    /// (`file_commits`) so the post-pass can distinguish symmetric pairs
    /// from one-way ones.
    fn absorb_commit(
        &self,
        repo: &Repository,
        commit: &git2::Commit<'_>,
        pair_counts: &mut HashMap<(PathBuf, PathBuf), u32>,
        file_commits: &mut HashMap<PathBuf, u32>,
    ) -> bool {
        let Ok(commit_tree) = commit.tree() else {
            return false;
        };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        else {
            return false;
        };

        let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
        for delta in diff.deltas() {
            let Some(path) = delta.new_file().path() else {
                continue;
            };
            if path.as_os_str().is_empty() || is_path_excluded(path, &self.excluded) {
                continue;
            }
            paths.insert(path.to_path_buf());
        }
        if paths.is_empty() || paths.len() > BULK_COMMIT_FILE_LIMIT {
            return false;
        }

        // file_commits counts every commit a file participated in, solo
        // included — that's the denominator `P(other | self)` needs to
        // tell a leader (frequently changes alone) apart from a
        // follower (always tags along with the partner).
        for path in &paths {
            let entry = file_commits.entry(path.clone()).or_insert(0);
            *entry = entry.saturating_add(1);
        }
        if paths.len() < 2 {
            // Solo commit: nothing to pair, but the file_commits bump
            // above is what makes symmetric vs one-way distinguishable.
            return true;
        }

        // BTreeSet iterates in sorted order, so the (a, b) pairs we emit are
        // already canonical (a < b).
        let ordered: Vec<&PathBuf> = paths.iter().collect();
        for (i, a) in ordered.iter().enumerate() {
            for b in &ordered[i + 1..] {
                let counter = pair_counts.entry(((*a).clone(), (*b).clone())).or_insert(0);
                *counter = counter.saturating_add(1);
            }
        }
        true
    }
}

#[must_use]
pub fn default_symmetric_threshold() -> f64 {
    0.5
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeCouplingReport {
    /// All co-occurring pairs whose count meets `min_coupling`, sorted by
    /// count descending then path lexicographically.
    pub pairs: Vec<FilePair>,
    /// Per-file sum-of-coupling: the sum of pair counts every pair this
    /// file participates in. Mirrors Tornhill's "sum of coupling" metric.
    pub file_sums: Vec<FileSum>,
    pub totals: CouplingTotals,
    pub since_days: u32,
    pub min_coupling: u32,
}

impl ChangeCouplingReport {
    /// Top-N pairs by co-occurrence count (descending). The underlying
    /// `pairs` vector is already sorted by `collect_pairs`.
    #[must_use]
    pub fn worst_n_pairs(&self, n: usize) -> Vec<FilePair> {
        let mut top = self.pairs.clone();
        top.truncate(n);
        top
    }

    /// Top-N files by sum-of-coupling (descending). The underlying
    /// `file_sums` vector is already sorted by `compute_file_sums`.
    #[must_use]
    pub fn worst_n_files(&self, n: usize) -> Vec<FileSum> {
        let mut top = self.file_sums.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilePair {
    /// Always the lexicographically smaller path of the pair.
    pub a: PathBuf,
    pub b: PathBuf,
    pub count: u32,
    /// Symmetry of the co-change pattern. `None` on legacy snapshots
    /// written before v0.2 added the symmetric / one-way split.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<PairDirection>,
    /// Pair-class taxonomy, applied post-collection so the v0.3 noise
    /// filter (`Lockfile` / `Generated` / `Manifest` are dropped before
    /// they reach the report) and the demote tier (`TestSrc` / `DocSrc`
    /// stay in the report but skip Finding emission, reserved for v0.4
    /// drift detection) work without touching the per-commit walk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<PairClass>,
}

/// Pair-class taxonomy. Built-in language-aware patterns drop pure-noise
/// pairs (`Lockfile` / `Generated` / `Manifest`) before they enter the
/// report; `TestSrc` / `DocSrc` pairs are preserved for future drift
/// detection but skipped from the drain queue.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PairClass {
    Genuine,
    TestSrc,
    DocSrc,
    Manifest,
    Lockfile,
    Generated,
}

/// Whether a co-changing pair behaves as a single unit (`Symmetric`)
/// or has a clear leader/follower (`OneWay`). Computed by the observer
/// from per-file commit counts; see the module-level docs for the
/// threshold semantics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PairDirection {
    /// Both `P(B|A)` and `P(A|B)` exceed `symmetric_threshold` — the
    /// pair almost never moves alone. The strongest "responsibility
    /// mixing" signal in this metric.
    Symmetric,
    /// Changes flow `from → to`. `from` often changes alone; `to`
    /// rarely does. Picked as the file whose changes the partner is
    /// most conditionally dependent on.
    OneWay { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSum {
    pub path: PathBuf,
    pub sum: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouplingTotals {
    pub pairs: usize,
    pub files: usize,
    pub commits_considered: u32,
}

impl Observer for ChangeCouplingObserver {
    type Output = ChangeCouplingReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "change_coupling",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}

impl IntoFindings for ChangeCouplingReport {
    /// `a` (lex-smaller, already canonical) is the primary
    /// `location.file`; the partner `b` is the primary's `symbol` (so
    /// a → b and a → c distinguish in `id`) and also a secondary entry
    /// in `locations`. Pairs with `direction = Some(Symmetric)` carry
    /// the metric tag `change_coupling.symmetric` so the renderer can
    /// surface the responsibility-mixing signal separately; everything
    /// else stays under `change_coupling`.
    ///
    /// Emits one Finding per pair regardless of class so the zip in
    /// `ChangeCouplingFeature::lower` stays index-aligned. The lower
    /// pass drops `TestSrc` / `DocSrc` Findings so they never reach
    /// the drain queue.
    fn into_findings(&self) -> Vec<Finding> {
        self.pairs
            .iter()
            .map(|pair| {
                let b_str = pair.b.to_string_lossy().into_owned();
                let primary = Location {
                    file: pair.a.clone(),
                    line: None,
                    symbol: Some(b_str.clone()),
                };
                let (metric, arrow) = render_metric_and_arrow(pair);
                let summary = format!(
                    "co-changed {} times: {} {arrow} {}",
                    pair.count,
                    pair.a.display(),
                    b_str,
                );
                Finding::new(metric, primary, summary, &format!("count:{}", pair.count))
                    .with_locations(vec![Location::file(pair.b.clone())])
            })
            .collect()
    }
}

/// Single match table for the rendered tag and arrow, so a future
/// direction variant only touches one site.
fn render_metric_and_arrow(pair: &FilePair) -> (&'static str, &'static str) {
    match &pair.direction {
        Some(PairDirection::Symmetric) => {
            (Finding::METRIC_CHANGE_COUPLING_SYMMETRIC, "↔ (symmetric)")
        }
        Some(PairDirection::OneWay { from, .. }) if from == &pair.a => {
            (Finding::METRIC_CHANGE_COUPLING, "→")
        }
        Some(PairDirection::OneWay { .. }) => (Finding::METRIC_CHANGE_COUPLING, "←"),
        None => (Finding::METRIC_CHANGE_COUPLING, "↔"),
    }
}

fn collect_pairs(
    pair_counts: HashMap<(PathBuf, PathBuf), u32>,
    min_coupling: u32,
    min_lift: f64,
    file_commits: &HashMap<PathBuf, u32>,
    commits_considered: u32,
    symmetric_threshold: f64,
) -> Vec<FilePair> {
    let mut pairs: Vec<FilePair> = pair_counts
        .into_iter()
        .filter(|(_, count)| *count >= min_coupling)
        .filter(|((a, b), count)| {
            let count_a = file_commits.get(a).copied().unwrap_or(0).max(*count);
            let count_b = file_commits.get(b).copied().unwrap_or(0).max(*count);
            lift(*count, count_a, count_b, commits_considered) >= min_lift
        })
        .map(|((a, b), count)| {
            let count_a = file_commits.get(&a).copied().unwrap_or(0).max(count);
            let count_b = file_commits.get(&b).copied().unwrap_or(0).max(count);
            let direction =
                classify_direction(&a, &b, count, count_a, count_b, symmetric_threshold);
            FilePair {
                a,
                b,
                count,
                direction: Some(direction),
                class: None,
            }
        })
        .collect();
    pairs.sort_by(|x, y| {
        y.count
            .cmp(&x.count)
            .then_with(|| x.a.cmp(&y.a))
            .then_with(|| x.b.cmp(&y.b))
    });
    pairs
}

/// Lift = `P(A∩B) / (P(A) × P(B))` — how much more often the pair
/// co-occurs than chance. `>1.0` means above-chance association,
/// `2.0` is the conventional "interesting" threshold in association-
/// rule mining. Returns `f64::INFINITY` when the universe is empty
/// (degenerate; the pair filter would have dropped the pair via
/// `min_coupling` long before this).
fn lift(pair_count: u32, count_a: u32, count_b: u32, commits_considered: u32) -> f64 {
    if commits_considered == 0 || count_a == 0 || count_b == 0 {
        return f64::INFINITY;
    }
    #[allow(clippy::cast_precision_loss)]
    {
        f64::from(pair_count) * f64::from(commits_considered)
            / (f64::from(count_a) * f64::from(count_b))
    }
}

/// Classify the pair from per-file totals. `count_a` / `count_b` are
/// the total commits each file participated in. The ratio
/// `pair_count / count_*` is `P(other | this)` — the probability that
/// "this" file's edits drag the partner along. Both above
/// `symmetric_threshold` → Symmetric; otherwise the file the partner
/// is more conditionally bound to is the leader (`from`).
fn classify_direction(
    a: &Path,
    b: &Path,
    pair_count: u32,
    count_a: u32,
    count_b: u32,
    symmetric_threshold: f64,
) -> PairDirection {
    #[allow(clippy::cast_precision_loss)]
    let p_b_given_a = f64::from(pair_count) / f64::from(count_a);
    #[allow(clippy::cast_precision_loss)]
    let p_a_given_b = f64::from(pair_count) / f64::from(count_b);
    if p_b_given_a >= symmetric_threshold && p_a_given_b >= symmetric_threshold {
        PairDirection::Symmetric
    } else if p_a_given_b > p_b_given_a {
        PairDirection::OneWay {
            from: a.to_path_buf(),
            to: b.to_path_buf(),
        }
    } else {
        PairDirection::OneWay {
            from: b.to_path_buf(),
            to: a.to_path_buf(),
        }
    }
}

fn compute_file_sums(pairs: &[FilePair]) -> Vec<FileSum> {
    let mut sums: BTreeMap<PathBuf, u32> = BTreeMap::new();
    for pair in pairs {
        let a = sums.entry(pair.a.clone()).or_insert(0);
        *a = a.saturating_add(pair.count);
        let b = sums.entry(pair.b.clone()).or_insert(0);
        *b = b.saturating_add(pair.count);
    }
    let mut file_sums: Vec<FileSum> = sums
        .into_iter()
        .map(|(path, sum)| FileSum { path, sum })
        .collect();
    file_sums.sort_by(|x, y| y.sum.cmp(&x.sum).then_with(|| x.path.cmp(&y.path)));
    file_sums
}

/// Apply [`PairClass`] tags to every pair in `report`, then drop pure-
/// noise classes (`Lockfile` / `Generated` / `Manifest`). `TestSrc` and
/// `DocSrc` pairs are kept (tagged) for v0.4 drift detection — the
/// `IntoFindings` lift will skip them so they don't enter the drain
/// queue. `primary_lang` comes from `LocReport::primary` and gates the
/// language-specific pattern bundle.
pub(crate) fn classify_and_filter(report: &mut ChangeCouplingReport, primary_lang: Option<&str>) {
    for pair in &mut report.pairs {
        pair.class = Some(classify_pair(&pair.a, &pair.b, primary_lang));
    }
    report.pairs.retain(|p| {
        !matches!(
            p.class,
            Some(PairClass::Lockfile | PairClass::Generated | PairClass::Manifest)
        )
    });
    report.file_sums = compute_file_sums(&report.pairs);
    report.totals.pairs = report.pairs.len();
    report.totals.files = report.file_sums.len();
}

/// Classify a pair against language-aware path patterns. Strongest
/// exclusions first (Lockfile / Generated win over any co-occurring
/// Doc or Test marker), then Manifest (mod.rs / index.ts / __init__.py
/// living next to a sibling), then any pair touching a test or doc
/// file, then Genuine.
///
/// `TestSrc` / `DocSrc` use OR semantics — any pair where at least one
/// side is a test or a doc demotes (e.g. doc ↔ doc EN/JA mirror is
/// expected hygiene; test ↔ test in a shared scaffold likewise).
fn classify_pair(a: &Path, b: &Path, primary_lang: Option<&str>) -> PairClass {
    let a_role = file_role(a, primary_lang);
    let b_role = file_role(b, primary_lang);

    if matches!(a_role, FileRole::Lockfile) || matches!(b_role, FileRole::Lockfile) {
        return PairClass::Lockfile;
    }
    if matches!(a_role, FileRole::Generated) || matches!(b_role, FileRole::Generated) {
        return PairClass::Generated;
    }
    if is_manifest_pair(a, b) {
        return PairClass::Manifest;
    }
    if matches!(a_role, FileRole::Test) || matches!(b_role, FileRole::Test) {
        return PairClass::TestSrc;
    }
    if matches!(a_role, FileRole::Doc) || matches!(b_role, FileRole::Doc) {
        return PairClass::DocSrc;
    }
    PairClass::Genuine
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileRole {
    Source,
    Test,
    Doc,
    Lockfile,
    Generated,
}

fn file_role(path: &Path, primary_lang: Option<&str>) -> FileRole {
    let path_str = path.to_string_lossy();
    let basename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();

    if is_lockfile(basename, primary_lang) {
        return FileRole::Lockfile;
    }
    if is_generated(&path_str, basename, primary_lang) {
        return FileRole::Generated;
    }
    if is_test(&path_str, basename) {
        return FileRole::Test;
    }
    if is_doc(&path_str, basename) {
        return FileRole::Doc;
    }
    FileRole::Source
}

// Lockfile / generated / test / doc filenames are convention, always
// lowercase in practice. The lint about case-sensitive extension
// comparison would force `eq_ignore_ascii_case` rituals that add no
// signal.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_lockfile(basename: &str, _primary_lang: Option<&str>) -> bool {
    // Generic suffixes (`*.lock`, `*.lockb`, `go.sum`) catch most
    // ecosystems. Well-known basenames are matched unconditionally
    // because monorepos commonly mix languages — a Rust workspace's
    // `docs/` may still carry a `package-lock.json`, and the primary
    // language detection is project-wide.
    if basename.ends_with(".lock") || basename.ends_with(".lockb") || basename == "go.sum" {
        return true;
    }
    matches!(
        basename,
        "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "bun.lock"
            | "bun.lockb"
            | "poetry.lock"
            | "Pipfile.lock"
            | "uv.lock"
            | "composer.lock"
            | "Gemfile.lock"
    )
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_generated(path_str: &str, basename: &str, primary_lang: Option<&str>) -> bool {
    // Cross-language directory markers — covers most tooling output.
    // Match both "<root>/dir/" (sub-path) and "dir/" at the start of the
    // path (no leading slash).
    const COMMON_DIRS: &[&str] = &[
        "dist/",
        "build/",
        "__generated__/",
        "generated/",
        "__pycache__/",
        "node_modules/",
        "vendor/",
    ];
    if dir_marker_matches(path_str, COMMON_DIRS) {
        return true;
    }
    // Generated artefacts that ship next to source instead of in dist/.
    if basename.ends_with(".min.js")
        || basename.ends_with(".min.css")
        || basename.contains(".bundle.")
        || basename.ends_with(".snap")
    {
        return true;
    }
    match primary_lang {
        Some("rust") => dir_marker_matches(path_str, &["target/"]),
        Some("python") => path_str.contains(".egg-info/"),
        _ => false,
    }
}

/// True iff any of `dirs` (each a `name/` form, no leading slash)
/// appears as a path component — either at the start of the string or
/// preceded by `/`.
fn dir_marker_matches(path_str: &str, dirs: &[&str]) -> bool {
    dirs.iter()
        .any(|d| path_str.starts_with(d) || path_str.contains(&format!("/{d}")))
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_test(path_str: &str, basename: &str) -> bool {
    // Suffix-based: `foo.test.ts`, `foo_test.go`, `test_foo.py`, `foo.spec.ts`.
    if basename.contains(".test.")
        || basename.contains(".spec.")
        || basename.starts_with("test_")
        || basename.ends_with("_test.go")
        || basename.ends_with("_test.rs")
        || basename.ends_with("_test.py")
    {
        return true;
    }
    dir_marker_matches(path_str, &["tests/", "__tests__/", "spec/", "test/"])
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_doc(path_str: &str, basename: &str) -> bool {
    if basename.ends_with(".md") || basename.ends_with(".mdx") || basename.ends_with(".rst") {
        return true;
    }
    dir_marker_matches(path_str, &["docs/"])
}

/// True iff the pair is a module manifest paired with a sibling — the
/// vertical "re-export" coupling that's a structural artefact, not a
/// design problem. Both files must share the same parent directory.
fn is_manifest_pair(a: &Path, b: &Path) -> bool {
    let (Some(a_parent), Some(b_parent)) = (a.parent(), b.parent()) else {
        return false;
    };
    if a_parent != b_parent {
        return false;
    }
    let a_name = a.file_name().and_then(|f| f.to_str()).unwrap_or_default();
    let b_name = b.file_name().and_then(|f| f.to_str()).unwrap_or_default();
    is_module_manifest(a_name) ^ is_module_manifest(b_name)
}

fn is_module_manifest(basename: &str) -> bool {
    matches!(
        basename,
        "mod.rs" | "lib.rs" | "main.rs" | "__init__.py" | "index.ts" | "index.tsx" | "index.js"
    )
}

pub struct ChangeCouplingFeature;

impl Feature for ChangeCouplingFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "change_coupling",
            version: 1,
            kind: FeatureKind::Observer,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.metrics.change_coupling.enabled
    }
    fn lower(
        &self,
        reports: &ObserverReports,
        cfg: &Config,
        cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(cc) = reports.change_coupling.as_ref() else {
            return Vec::new();
        };
        let workspaces = cfg.project.workspaces.as_slice();
        let cross_policy = cfg.metrics.change_coupling.cross_workspace;
        let mut out = Vec::with_capacity(cc.pairs.len());
        for (pair, mut finding) in cc.pairs.iter().zip(cc.into_findings()) {
            // TestSrc / DocSrc pairs stay in the report (v0.4 drift
            // detection consumes them) but don't enter the drain queue —
            // co-changing tests/docs is the expected hygiene, not a defect.
            if matches!(pair.class, Some(PairClass::TestSrc | PairClass::DocSrc)) {
                continue;
            }
            // A pair is cross-workspace iff both files resolve to a
            // declared workspace AND the two workspaces differ. Files
            // outside every workspace are *not* cross-workspace (they
            // belong to the implicit global cohort). `ws_a` doubles as
            // the calibration table key below — `pair.a` is also the
            // finding's canonical site (see `IntoFindings`).
            let ws_a = (!workspaces.is_empty())
                .then(|| crate::core::config::assign_workspace(&pair.a, workspaces))
                .flatten();
            let cross = ws_a.is_some_and(|a| {
                crate::core::config::assign_workspace(&pair.b, workspaces).is_some_and(|b| a != b)
            });
            if cross {
                match cross_policy {
                    crate::core::config::CrossWorkspacePolicy::Hide => continue,
                    crate::core::config::CrossWorkspacePolicy::Surface => {
                        // Retag so the drain policy can route it to its
                        // own bucket. Default policy parks
                        // `change_coupling.cross_workspace` in Advisory.
                        finding.metric = Finding::METRIC_CHANGE_COUPLING_CROSS_WORKSPACE.into();
                        finding.id = Finding::make_id(
                            &finding.metric,
                            &finding.location,
                            &format!("count:{}", pair.count),
                        );
                    }
                }
            }
            // Calibration follows the finding's primary site (`pair.a`).
            // We already resolved its workspace above — reuse rather
            // than walk the path components a second time.
            let cal_cc = cal.metrics_for_workspace(ws_a).change_coupling.as_ref();
            let severity = cal_cc.map_or(Severity::Ok, |c| c.classify(f64::from(pair.count)));
            out.push(decorate(finding, severity, hotspot));
        }
        out
    }
}

#[cfg(test)]
mod pair_class_tests {
    use super::*;

    fn pair(a: &str, b: &str, count: u32) -> FilePair {
        // Canonical: a is the lex-smaller path (matches collect_pairs).
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        FilePair {
            a: PathBuf::from(a),
            b: PathBuf::from(b),
            count,
            direction: None,
            class: None,
        }
    }

    fn report(pairs: Vec<FilePair>) -> ChangeCouplingReport {
        ChangeCouplingReport {
            pairs,
            file_sums: Vec::new(),
            totals: CouplingTotals::default(),
            since_days: 90,
            min_coupling: 3,
        }
    }

    #[test]
    fn lockfile_pair_dropped_for_typescript_project() {
        let mut r = report(vec![
            pair("package.json", "bun.lock", 5),
            pair("src/foo.ts", "src/bar.ts", 4),
        ]);
        classify_and_filter(&mut r, Some("typescript"));
        assert_eq!(r.pairs.len(), 1, "lockfile pair must be dropped");
        assert_eq!(r.pairs[0].class, Some(PairClass::Genuine));
    }

    #[test]
    fn generic_lock_suffix_matches_any_language() {
        // Cargo.lock + target/ both flagged for a Rust project.
        let mut r = report(vec![
            pair("Cargo.lock", "src/lib.rs", 5),
            pair("target/debug/build", "src/lib.rs", 5),
        ]);
        classify_and_filter(&mut r, Some("rust"));
        assert_eq!(r.pairs.len(), 0, "lockfile + target/ both dropped");
    }

    #[test]
    fn dist_artefact_pair_dropped() {
        let mut r = report(vec![
            pair("src/index.ts", "dist/index.css", 8),
            pair("src/foo.ts", "src/bar.ts", 4),
        ]);
        classify_and_filter(&mut r, Some("typescript"));
        assert_eq!(r.pairs.len(), 1);
        assert_eq!(r.pairs[0].a, PathBuf::from("src/bar.ts"));
    }

    #[test]
    fn doc_pair_demoted_not_dropped() {
        let mut r = report(vec![
            pair("CLAUDE.md", "src/lib.rs", 7),
            pair("src/foo.rs", "src/bar.rs", 4),
        ]);
        classify_and_filter(&mut r, Some("rust"));
        assert_eq!(r.pairs.len(), 2, "DocSrc pairs stay in the report");
        let doc_pair = r
            .pairs
            .iter()
            .find(|p| p.a == Path::new("CLAUDE.md") || p.b == Path::new("CLAUDE.md"))
            .expect("doc pair preserved");
        assert_eq!(doc_pair.class, Some(PairClass::DocSrc));
    }

    #[test]
    fn test_pair_demoted_not_dropped() {
        let mut r = report(vec![pair("src/foo.test.ts", "src/foo.ts", 6)]);
        classify_and_filter(&mut r, Some("typescript"));
        assert_eq!(r.pairs.len(), 1);
        assert_eq!(r.pairs[0].class, Some(PairClass::TestSrc));
    }

    #[test]
    fn manifest_pair_dropped() {
        // mod.rs paired with sibling in same dir = vertical re-export.
        let mut r = report(vec![pair(
            "crates/cli/src/observer/mod.rs",
            "crates/cli/src/observer/loc.rs",
            5,
        )]);
        classify_and_filter(&mut r, Some("rust"));
        assert_eq!(r.pairs.len(), 0, "mod.rs ↔ sibling dropped as Manifest");
    }

    #[test]
    fn manifest_pair_requires_same_directory() {
        // mod.rs in a different directory is NOT a manifest pair.
        let mut r = report(vec![pair(
            "crates/cli/src/observer/mod.rs",
            "crates/cli/src/cli.rs",
            5,
        )]);
        classify_and_filter(&mut r, Some("rust"));
        assert_eq!(r.pairs.len(), 1);
        assert_eq!(r.pairs[0].class, Some(PairClass::Genuine));
    }

    #[test]
    fn into_findings_emits_for_all_pairs_lower_filters_demoted() {
        // The report carries a Genuine + DocSrc pair after filter.
        let mut r = report(vec![
            pair("README.md", "src/lib.rs", 6),
            pair("src/foo.rs", "src/bar.rs", 5),
        ]);
        classify_and_filter(&mut r, Some("rust"));
        // into_findings emits one per surviving pair (zip alignment).
        let findings = r.into_findings();
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn python_specific_lockfiles() {
        for lf in ["poetry.lock", "Pipfile.lock", "uv.lock"] {
            let mut r = report(vec![pair(lf, "src/main.py", 5)]);
            classify_and_filter(&mut r, Some("python"));
            assert_eq!(r.pairs.len(), 0, "{lf} must be dropped");
        }
    }

    #[test]
    fn unknown_language_still_drops_generic_lockfiles() {
        // Generic *.lock suffix triggers regardless of primary_lang.
        let mut r = report(vec![pair("foo.lock", "src/main.kt", 5)]);
        classify_and_filter(&mut r, None);
        assert_eq!(r.pairs.len(), 0);
    }

    #[test]
    fn lift_above_chance() {
        // 5 co-occurrences out of 10 commits where each file appears
        // 5 times: lift = 5×10 / (5×5) = 2.0 — at the threshold.
        assert!((lift(5, 5, 5, 10) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn lift_below_chance_for_widespread_files() {
        // Each file in 50 of 100 commits, 5 overlap: lift = 5×100/(50×50) = 0.2.
        assert!((lift(5, 50, 50, 100) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn lift_handles_empty_universe() {
        // Degenerate inputs return INFINITY so the filter never drops the
        // pair on a math edge case (min_coupling will have caught it first).
        assert!(lift(0, 0, 0, 0).is_infinite());
        assert!(lift(5, 0, 5, 100).is_infinite());
    }

    mod cross_workspace_lower {
        use super::*;
        use crate::core::calibration::Calibration;
        use crate::core::config::{Config, CrossWorkspacePolicy, WorkspaceOverlay};
        use crate::feature::{Feature, HotspotIndex};
        use crate::observers::ObserverReports;

        fn cfg(workspaces: Vec<&str>, policy: CrossWorkspacePolicy) -> Config {
            let mut c = Config::default();
            c.metrics.change_coupling.enabled = true;
            c.metrics.change_coupling.cross_workspace = policy;
            c.project.workspaces = workspaces
                .into_iter()
                .map(|p| WorkspaceOverlay {
                    path: p.into(),
                    primary_language: None,
                    exclude_paths: Vec::new(),
                })
                .collect();
            c
        }

        fn reports_with(pairs: Vec<FilePair>) -> ObserverReports {
            ObserverReports {
                loc: crate::observer::loc::LocReport::default(),
                complexity: crate::observer::complexity::ComplexityReport::default(),
                complexity_observer: crate::observer::complexity::ComplexityObserver::default(),
                churn: None,
                change_coupling: Some(report(pairs)),
                duplication: None,
                hotspot: None,
                lcom: None,
            }
        }

        #[test]
        fn cross_workspace_pair_retagged_when_surface() {
            let pairs = vec![pair("packages/api/server.ts", "packages/web/client.ts", 5)];
            let r = reports_with(pairs);
            let c = cfg(
                vec!["packages/api", "packages/web"],
                CrossWorkspacePolicy::Surface,
            );
            let f = ChangeCouplingFeature.lower(
                &r,
                &c,
                &Calibration::default(),
                &HotspotIndex::default(),
            );
            assert_eq!(f.len(), 1);
            assert_eq!(f[0].metric, "change_coupling.cross_workspace");
            // id rebuilt from the new metric tag.
            assert!(f[0].id.starts_with("change_coupling.cross_workspace:"));
        }

        #[test]
        fn cross_workspace_pair_dropped_when_hide() {
            let pairs = vec![
                pair("packages/api/server.ts", "packages/web/client.ts", 5),
                pair("packages/api/a.ts", "packages/api/b.ts", 5),
            ];
            let r = reports_with(pairs);
            let c = cfg(
                vec!["packages/api", "packages/web"],
                CrossWorkspacePolicy::Hide,
            );
            let f = ChangeCouplingFeature.lower(
                &r,
                &c,
                &Calibration::default(),
                &HotspotIndex::default(),
            );
            assert_eq!(f.len(), 1, "only the same-workspace pair survives");
            assert_eq!(f[0].metric, "change_coupling");
        }

        #[test]
        fn pair_with_one_unscoped_file_not_cross_workspace() {
            // One file lives outside every declared workspace —
            // treat as same-workspace (no special tagging).
            let pairs = vec![pair("packages/api/server.ts", "scripts/ci.sh", 5)];
            let r = reports_with(pairs);
            let c = cfg(vec!["packages/api"], CrossWorkspacePolicy::Surface);
            let f = ChangeCouplingFeature.lower(
                &r,
                &c,
                &Calibration::default(),
                &HotspotIndex::default(),
            );
            assert_eq!(f.len(), 1);
            assert_eq!(f[0].metric, "change_coupling");
        }

        #[test]
        fn no_workspaces_declared_means_no_cross_workspace_tag() {
            let pairs = vec![pair("a/x.ts", "b/y.ts", 5)];
            let r = reports_with(pairs);
            let mut c = Config::default();
            c.metrics.change_coupling.enabled = true;
            let f = ChangeCouplingFeature.lower(
                &r,
                &c,
                &Calibration::default(),
                &HotspotIndex::default(),
            );
            assert_eq!(f.len(), 1);
            assert_eq!(f[0].metric, "change_coupling");
        }
    }
}
