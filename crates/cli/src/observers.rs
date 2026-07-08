//! Single source of truth for "run every enabled observer". `heal
//! status`, `heal diff`, and the post-commit nudge all funnel through
//! [`run_all`] / [`build_record`] so a new observer or enable-flag
//! only needs editing in one place.

use std::collections::BTreeMap;
use std::path::Path;

use crate::core::calibration::{
    Calibration, CalibrationMeta, HotspotCalibration, MetricCalibration, MetricCalibrations,
    MetricFloors, FLOOR_CCN, FLOOR_COGNITIVE, FLOOR_DUPLICATION_PCT, FLOOR_OK_CCN,
    FLOOR_OK_COGNITIVE, FLOOR_OK_DOC_HOTSPOT, FLOOR_OK_TEST_HOTSPOT, STRATEGY_PERCENTILE,
};
use crate::core::config::Config;
use crate::core::doc_pairs::DocPairsFile;
use crate::core::finding::Finding;
use crate::observer::code::change_coupling::{ChangeCouplingObserver, ChangeCouplingReport};
use crate::observer::code::churn::{ChurnObserver, ChurnReport};
use crate::observer::code::complexity::{ComplexityObserver, ComplexityReport};
use crate::observer::code::duplication::{
    DocsDuplicationInputs, DuplicationObserver, DuplicationReport,
};
use crate::observer::code::hotspot::{compose as compose_hotspot, HotspotReport, HotspotWeights};
use crate::observer::code::lcom::{LcomObserver, LcomReport};
use crate::observer::code::loc::{LocObserver, LocReport};
use crate::observer::docs::coverage::{DocCoverageObserver, DocCoverageReport};
use crate::observer::docs::drift::{DocDriftObserver, DocDriftReport};
use crate::observer::docs::freshness::{DocFreshnessObserver, DocFreshnessReport};
use crate::observer::docs::hotspot::{compose as compose_doc_hotspot, DocHotspotReport};
use crate::observer::docs::link_health::{
    paired_doc_paths, DocLinkHealthObserver, DocLinkHealthReport,
};
use crate::observer::docs::orphan_pages::{OrphanPagesObserver, OrphanPagesReport};
use crate::observer::docs::todo_density::{TodoDensityObserver, TodoDensityReport};
use crate::observer::docs::walk::walk_standalone_docs;
use crate::observer::test::coverage::{CoverageObserver, CoverageReport};
use crate::observer::test::hotspot::{compose as compose_test_hotspot, TestHotspotReport};
use crate::observer::test::skip_ratio::{SkipRatioObserver, SkipRatioReport};

use crate::cli::MetricKind;

#[derive(Default)]
pub struct ObserverReports {
    pub loc: LocReport,
    pub complexity: ComplexityReport,
    pub complexity_observer: ComplexityObserver,
    pub churn: Option<ChurnReport>,
    pub change_coupling: Option<ChangeCouplingReport>,
    pub duplication: Option<DuplicationReport>,
    pub hotspot: Option<HotspotReport>,
    pub lcom: Option<LcomReport>,
    /// `[features.docs]` `SSoT` loaded from `.heal/doc_pairs.json`.
    /// `None` whenever the feature is disabled or the file is absent
    /// — docs observers down-stream return empty reports so the rest
    /// of the pipeline ignores them.
    pub doc_pairs: Option<DocPairsFile>,
    /// Per-pair "src commits since doc" output from
    /// [`DocFreshnessObserver`]. `None` when the docs feature is off
    /// or `doc_pairs` is absent.
    pub doc_freshness: Option<DocFreshnessReport>,
    /// Dangling identifiers detected by [`DocDriftObserver`]. Same
    /// gating as [`Self::doc_freshness`].
    pub doc_drift: Option<DocDriftReport>,
    /// Pairs whose `doc` no longer exists on disk. Same gating as
    /// [`Self::doc_freshness`].
    pub doc_coverage: Option<DocCoverageReport>,
    /// Broken internal links across Layer A + Layer B docs. External
    /// HTTP checks are out of scope (R5).
    pub doc_link_health: Option<DocLinkHealthReport>,
    /// Layer B docs that no other doc references.
    pub orphan_pages: Option<OrphanPagesReport>,
    /// Per-doc TODO/FIXME/XXX/TBD/[要確認] marker counts.
    pub todo_density: Option<TodoDensityReport>,
    /// Per-source-file line coverage parsed from an externally-generated
    /// lcov.info file. `None` whenever `[features.test.coverage]` is
    /// disabled, the user is running a single-metric scan that doesn't
    /// need it, or the configured `lcov_paths` resolve to nothing on
    /// disk.
    pub coverage: Option<CoverageReport>,
    /// Per-test-file skipped-test ratio. `None` whenever
    /// `[features.test]` is disabled or no `test_paths` are configured.
    pub skip_ratio: Option<SkipRatioReport>,
    /// Per-src-file `commits × uncov_pct` composite. `None` unless
    /// the user asked for the metric or `[features.test.coverage]` is
    /// enabled (the per-family hotspot decoration uses this index).
    pub test_hotspot: Option<TestHotspotReport>,
    /// Per-pair `paired_src_churn × debt` composite. `None` unless
    /// the user asked for the metric or `[features.docs]` is enabled.
    pub doc_hotspot: Option<DocHotspotReport>,
}

/// Run the observers needed for the requested metric. `only = None`
/// means "run everything" (the default, used by `heal status` and
/// the post-commit nudge). When `only` is set, observers irrelevant
/// to that metric are skipped —
/// churn and complexity still run for `Hotspot` because the composite
/// is built on top of them. The skipped observers' fields fall back to
/// `Default` (or `None` for the optional ones).
///
/// `workspace = Some(path)` (used by `heal metrics --workspace`)
/// scopes every observer's internal walk or git diff to files under
/// the sub-path, so totals, pair counts, and lift reflect only the
/// chosen workspace's universe. Pass `None` for whole-repo behavior.
#[allow(clippy::too_many_lines)] // each observer is one cheap branch; flat reads better than splitting
pub(crate) fn run_all(
    project: &Path,
    cfg: &Config,
    only: Option<MetricKind>,
    workspace: Option<&Path>,
) -> ObserverReports {
    let want = |m: MetricKind| match only {
        None => true,
        Some(o) if o == m => true,
        Some(MetricKind::Hotspot) if matches!(m, MetricKind::Churn | MetricKind::Complexity) => {
            true
        }
        // test_hotspot is `commits × uncov_pct` — needs both Churn and
        // CoveragePct as inputs, so a `--metric test-hotspot` run pulls
        // them in even when neither was named.
        Some(MetricKind::TestHotspot)
            if matches!(m, MetricKind::Churn | MetricKind::CoveragePct) =>
        {
            true
        }
        // doc_hotspot needs Churn (paired-src volatility) plus
        // DocFreshness (staleness) plus DocDrift (dangling idents).
        Some(MetricKind::DocHotspot)
            if matches!(
                m,
                MetricKind::Churn | MetricKind::DocFreshness | MetricKind::DocDrift
            ) =>
        {
            true
        }
        // doc_freshness reads `.heal/doc_pairs.json` — independent of
        // every other observer, so the filter is the simple equality
        // case above. No cross-metric implication needed.
        _ => false,
    };

    let ws_buf = workspace.map(Path::to_path_buf);
    // Loc bypasses the per-observer `workspace` field — tokei is
    // pointed at the workspace dir directly so it walks only that
    // sub-tree. `.gitignore` resolution still climbs to project root
    // as before via `WalkBuilder::require_git(false)`.
    let loc_root = workspace.unwrap_or(project);
    let loc = if want(MetricKind::Loc) {
        LocObserver::from_config(cfg).scan(loc_root)
    } else {
        LocReport::default()
    };
    let complexity_observer = ComplexityObserver::from_config(cfg).with_workspace(ws_buf.clone());
    let churn = (want(MetricKind::Churn) && cfg.metrics.is_enabled("churn")).then(|| {
        ChurnObserver::from_config(cfg)
            .with_workspace(ws_buf.clone())
            .scan(project)
    });
    let change_coupling = (want(MetricKind::ChangeCoupling)
        && cfg.metrics.is_enabled("change_coupling"))
    .then(|| {
        ChangeCouplingObserver::from_config(cfg)
            .with_workspace(ws_buf.clone())
            .scan(project)
    })
    .map(|mut report| {
        crate::observer::code::change_coupling::classify_and_filter(
            &mut report,
            loc.primary.as_deref(),
        );
        report
    });
    // Docs prep is gated on whether any consumer was actually
    // requested — `heal metrics --metric ccn` shouldn't pay
    // ~300 `stat()` calls + the doc-body I/O when nothing
    // downstream will read the result. Pre-PR1 the boost path
    // forced this prep on every `heal status`; once boosts went
    // away the gate became user-driven.
    let want_doc_pairs_consumer = cfg.features.docs.enabled
        && (want(MetricKind::DocFreshness)
            || want(MetricKind::DocDrift)
            || want(MetricKind::DocCoverage)
            || want(MetricKind::OrphanPages)
            || want(MetricKind::DocHotspot));
    let want_doc_corpus_consumer = cfg.features.docs.enabled
        && (want(MetricKind::Duplication)
            || want(MetricKind::DocLinkHealth)
            || want(MetricKind::OrphanPages)
            || want(MetricKind::TodoDensity));
    let want_docs_prep = want_doc_pairs_consumer || want_doc_corpus_consumer;
    let doc_pairs = if want_docs_prep {
        load_doc_pairs(project, cfg)
    } else {
        None
    };
    let standalone_docs: Vec<std::path::PathBuf> = if want_doc_corpus_consumer {
        walk_standalone_docs(project, cfg)
    } else {
        Vec::new()
    };
    // Read every Layer A + Layer B doc body once. Four observers (link
    // health, orphans, TODO density, duplication MD pass) all want the
    // same bodies; the per-observer `fs::read_to_string` walks paid
    // 4× I/O on every `heal status`.
    let paired_doc_paths_owned = paired_doc_paths(doc_pairs.as_ref());
    let mut all_doc_paths: Vec<std::path::PathBuf> = standalone_docs.clone();
    for p in &paired_doc_paths_owned {
        if !all_doc_paths.contains(p) {
            all_doc_paths.push(p.clone());
        }
    }
    let doc_corpus: Vec<crate::observer::docs::corpus::DocBody> = if want_doc_corpus_consumer {
        crate::observer::docs::corpus::read_doc_bodies(project, &all_doc_paths)
    } else {
        Vec::new()
    };
    // One shared walk feeds Complexity, Duplication, and LCOM (see
    // `observer::code::scan_source_tree`): the three observers visit
    // the identical file universe (same excludes, same workspace
    // scope), and per-file tree-sitter parsing dominates their cost —
    // scanning them separately paid that parse up to three times.
    let duplication_observer =
        (want(MetricKind::Duplication) && cfg.metrics.is_enabled("duplication")).then(|| {
            let docs_inputs = if cfg.features.docs.enabled && !standalone_docs.is_empty() {
                Some(DocsDuplicationInputs {
                    min_tokens: cfg.metrics.duplication.docs_min_tokens,
                    docs: crate::observer::docs::corpus::select(&doc_corpus, &standalone_docs),
                })
            } else {
                None
            };
            DuplicationObserver::from_config(cfg)
                .with_workspace(ws_buf.clone())
                .with_docs(docs_inputs)
        });
    let lcom_observer = (want(MetricKind::Lcom) && cfg.metrics.is_enabled("lcom"))
        .then(|| LcomObserver::from_config(cfg).with_workspace(ws_buf.clone()));
    let mut complexity_acc = if want(MetricKind::Complexity) {
        complexity_observer.accumulator()
    } else {
        None
    };
    let mut duplication_acc = duplication_observer
        .as_ref()
        .and_then(DuplicationObserver::accumulator);
    let mut lcom_acc = lcom_observer.as_ref().and_then(LcomObserver::accumulator);
    crate::observer::code::scan_source_tree(
        project,
        &complexity_observer.excluded,
        ws_buf.as_deref(),
        complexity_acc.as_mut(),
        duplication_acc.as_mut(),
        lcom_acc.as_mut(),
    );
    let complexity = complexity_acc.map_or_else(
        ComplexityReport::default,
        crate::observer::code::complexity::ComplexityAccumulator::finish,
    );
    let duplication = duplication_observer.map(|o| match duplication_acc.take() {
        Some(acc) => o.finish(acc),
        // Enabled but zero window: `scan` returns the shell report.
        None => crate::observer::code::duplication::DuplicationReport {
            min_tokens: o.min_tokens,
            ..Default::default()
        },
    });
    let lcom = lcom_observer.map(|o| o.finish(lcom_acc.take().unwrap_or_default()));
    // Resolve the live pair list once and share across every Layer A
    // observer. `live_pairs` does an existence check per doc + per src,
    // so calling it three times (freshness, drift, coverage) burned
    // ~300 stat() calls on a 50-pair project.
    let live_pairs: Vec<_> = if want_doc_pairs_consumer {
        crate::observer::docs::freshness::live_pairs(doc_pairs.as_ref(), project)
    } else {
        Vec::new()
    };
    let coverage = (want(MetricKind::CoveragePct)
        && cfg.features.test.enabled
        && cfg.features.test.coverage.enabled)
        .then(|| CoverageObserver::from_config(cfg).scan(project));
    let skip_ratio = (want(MetricKind::SkipRatio) && cfg.features.test.enabled)
        .then(|| SkipRatioObserver::from_config(cfg).scan(project));
    let hotspot = match (
        want(MetricKind::Hotspot) && cfg.metrics.is_enabled("hotspot"),
        churn.as_ref(),
    ) {
        (true, Some(ch)) => Some(compose_hotspot(
            ch,
            &complexity,
            HotspotWeights {
                churn: cfg.metrics.hotspot.weight_churn,
                complexity: cfg.metrics.hotspot.weight_complexity,
            },
        )),
        _ => None,
    };
    let doc_freshness = (want(MetricKind::DocFreshness) && doc_pairs.is_some()).then(|| {
        DocFreshnessObserver::from_config_and_pairs(cfg, live_pairs.clone()).scan(project)
    });
    let doc_drift = (want(MetricKind::DocDrift) && doc_pairs.is_some())
        .then(|| DocDriftObserver::from_config_and_pairs(cfg, live_pairs.clone()).scan(project));
    let doc_coverage = (want(MetricKind::DocCoverage) && doc_pairs.is_some()).then(|| {
        // doc_coverage runs against the *raw* pair list — the whole
        // point is to surface pairs whose `doc` is missing on disk.
        let pairs = doc_pairs
            .as_ref()
            .map(|f| f.pairs.clone())
            .unwrap_or_default();
        DocCoverageObserver::from_config_and_pairs(cfg, pairs).scan(project)
    });
    let doc_link_health = (want(MetricKind::DocLinkHealth) && cfg.features.docs.enabled)
        .then(|| DocLinkHealthObserver::from_inputs(cfg, doc_corpus.clone()).scan(project));
    let orphan_pages = (want(MetricKind::OrphanPages) && cfg.features.docs.enabled).then(|| {
        let standalone = crate::observer::docs::corpus::select(&doc_corpus, &standalone_docs);
        OrphanPagesObserver::from_inputs(cfg, standalone, paired_doc_paths_owned.clone()).scan()
    });
    let todo_density = (want(MetricKind::TodoDensity) && cfg.features.docs.enabled).then(|| {
        // todo_density scans every Layer B doc plus every Layer A doc
        // — author-confessed incompleteness is interesting on both. The
        // corpus already holds the union; pass it through directly.
        TodoDensityObserver::from_inputs(cfg, doc_corpus.clone()).scan()
    });
    let test_hotspot = (want(MetricKind::TestHotspot)
        && cfg.features.test.enabled
        && cfg.features.test.coverage.enabled)
        .then(|| {
            // Universe-completion is load-bearing here — files that
            // the lcov reporter dropped are the most important hot
            // candidates, and they only enter the universe via
            // ChurnReport. The `want()` extension ensures churn ran.
            let ch = churn.as_ref();
            let cov = coverage.as_ref();
            ch.map(|ch| compose_test_hotspot(ch, cov))
        })
        .flatten();
    let doc_hotspot =
        (want(MetricKind::DocHotspot) && cfg.features.docs.enabled && doc_pairs.is_some())
            .then(|| {
                let ch = churn.as_ref()?;
                let fr = doc_freshness.as_ref()?;
                let dr = doc_drift.as_ref();
                Some(compose_doc_hotspot(
                    ch,
                    fr,
                    dr,
                    cfg.features.docs.hotspot.weight_drift,
                ))
            })
            .flatten();
    ObserverReports {
        loc,
        complexity,
        complexity_observer,
        churn,
        change_coupling,
        duplication,
        hotspot,
        lcom,
        doc_pairs,
        doc_freshness,
        doc_drift,
        doc_coverage,
        doc_link_health,
        orphan_pages,
        todo_density,
        coverage,
        skip_ratio,
        test_hotspot,
        doc_hotspot,
    }
}

/// Read `.heal/doc_pairs.json` (or whatever `cfg.features.docs.pairs_path`
/// resolves to) and surface any integrity issues to stderr.
///
/// `Ok(None)` from the reader means the file is absent — emit a one-line
/// hint pointing the user at `/heal-doc-pair-setup` (R3 forbids
/// auto-generation). A hard parse error is also folded into a warning
/// rather than aborting `heal status`; the user still sees the rest of
/// the findings.
fn load_doc_pairs(project: &Path, cfg: &Config) -> Option<DocPairsFile> {
    let pairs_path = &cfg.features.docs.pairs_path;
    match DocPairsFile::read(project, pairs_path) {
        Ok(Some(file)) => {
            for warning in file.integrity_check(project) {
                eprintln!(
                    "warn: {pairs_path}: pair[{}] references missing path {}",
                    warning.pair_index,
                    warning.missing_path.display(),
                );
            }
            Some(file)
        }
        Ok(None) => {
            eprintln!(
                "warn: {pairs_path} not found — run `claude /heal-doc-pair-setup` to generate it"
            );
            None
        }
        Err(err) => {
            eprintln!("warn: {pairs_path}: {err}");
            None
        }
    }
}

/// Build a Calibration from a fresh scan's reports. Distribution
/// inputs are per-finding-eligible measurements (per-function CCN /
/// Cognitive, per-file duplicate%, per-pair coupling count, per-file
/// hotspot score). Metrics whose observer didn't run, or whose
/// distribution is empty, omit their entry — `Calibration::classify`
/// then short-circuits to `Severity::Ok` for missing metrics.
///
/// Caller supplies `&Config` so disabled metrics skip calibration even
/// when the underlying complexity scan still produced raw values
/// (CCN/Cognitive share parsing, so both arrive together).
///
/// When `[[project.workspaces]]` is non-empty, the global
/// `Calibration::calibration` table holds breaks for files **outside**
/// every declared workspace (the leftover cohort) and a per-workspace
/// table is added under `Calibration::workspaces.<path>`. With no
/// workspaces declared, the global table holds the whole-repo
/// distribution exactly as before.
pub(crate) fn build_calibration(
    project: &Path,
    reports: &ObserverReports,
    config: &Config,
) -> Calibration {
    let workspaces = &config.project.workspaces;

    let global_filter = |file: &Path| -> bool {
        // No workspaces → whole repo is one cohort.
        // With workspaces → only count files outside every declared one.
        workspaces.is_empty() || crate::core::config::assign_workspace(file, workspaces).is_none()
    };
    let global_metrics = build_metric_calibrations(reports, config, &global_filter);

    let mut workspace_metrics: BTreeMap<String, MetricCalibrations> = BTreeMap::new();
    for ws in workspaces {
        let ws_path = ws.path.trim_end_matches('/').to_string();
        let in_workspace = |file: &Path| -> bool {
            crate::core::config::assign_workspace(file, workspaces).is_some_and(|w| w == ws_path)
        };
        let table = build_metric_calibrations(reports, config, &in_workspace);
        if has_any_table(&table) {
            workspace_metrics.insert(ws_path, table);
        }
    }

    let codebase_files = u32::try_from(
        reports
            .complexity
            .totals
            .files
            .max(reports.loc.total_files()),
    )
    .unwrap_or(u32::MAX);

    Calibration {
        meta: CalibrationMeta {
            created_at: chrono::Utc::now(),
            codebase_files,
            strategy: STRATEGY_PERCENTILE.to_owned(),
            calibrated_at_sha: crate::observer::shared::git::head_sha(project),
        },
        calibration: global_metrics,
        workspaces: workspace_metrics,
    }
    .with_overrides(config)
}

/// Produce a [`MetricCalibrations`] table from the subset of `reports`
/// values whose owning file passes `file_filter`. The same logic backs
/// both the global cohort and each per-workspace cohort — only the
/// filter changes.
#[allow(clippy::too_many_lines)] // each metric is one cheap branch; flat reads better than splitting
fn build_metric_calibrations(
    reports: &ObserverReports,
    config: &Config,
    file_filter: &dyn Fn(&Path) -> bool,
) -> MetricCalibrations {
    let ccn_floors = MetricFloors {
        critical: Some(FLOOR_CCN),
        ok: Some(FLOOR_OK_CCN),
    };
    let cognitive_floors = MetricFloors {
        critical: Some(FLOOR_COGNITIVE),
        ok: Some(FLOOR_OK_COGNITIVE),
    };
    let duplication_floors = MetricFloors {
        critical: Some(FLOOR_DUPLICATION_PCT),
        ok: None,
    };

    let ccn = if config.metrics.is_enabled("ccn") {
        let values: Vec<f64> = reports
            .complexity
            .files
            .iter()
            .filter(|f| file_filter(&f.path))
            .flat_map(|f| f.functions.iter().map(|fun| f64::from(fun.ccn)))
            .collect();
        non_empty(&values).then(|| MetricCalibration::from_distribution(&values, ccn_floors))
    } else {
        None
    };

    let cognitive = if config.metrics.is_enabled("cognitive") {
        let values: Vec<f64> = reports
            .complexity
            .files
            .iter()
            .filter(|f| file_filter(&f.path))
            .flat_map(|f| f.functions.iter().map(|fun| f64::from(fun.cognitive)))
            .collect();
        non_empty(&values).then(|| MetricCalibration::from_distribution(&values, cognitive_floors))
    } else {
        None
    };

    let duplication = reports.duplication.as_ref().and_then(|d| {
        let values: Vec<f64> = d
            .files
            .iter()
            .filter(|f| file_filter(&f.path))
            .map(|f| f.duplicate_pct)
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, duplication_floors))
    });

    // change_coupling: `min_coupling` already gates pairs at scan time so
    // the absolute floor is rare in practice. lcom: `min_cluster_count`
    // serves the same role. A pair contributes to a cohort iff **both**
    // files pass the filter — cross-cohort pairs are out of scope here
    // (PR4 surfaces them in their own Advisory bucket).
    let change_coupling = reports.change_coupling.as_ref().and_then(|c| {
        let values: Vec<f64> = c
            .pairs
            .iter()
            .filter(|p| file_filter(&p.a) && file_filter(&p.b))
            .map(|p| f64::from(p.count))
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, MetricFloors::default()))
    });

    let hotspot = reports.hotspot.as_ref().and_then(|h| {
        let scores: Vec<f64> = h
            .entries
            .iter()
            .filter(|e| file_filter(&e.path))
            .map(|e| e.score)
            .collect();
        non_empty(&scores).then(|| HotspotCalibration::from_distribution(&scores))
    });

    let lcom = reports.lcom.as_ref().and_then(|l| {
        let values: Vec<f64> = l
            .classes
            .iter()
            .filter(|c| file_filter(&c.file))
            .map(|c| f64::from(c.cluster_count))
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, MetricFloors::default()))
    });

    // Coverage stores **inverted** values (`100 - coverage_pct`) so the
    // existing `value >= p95 → Critical` cascade in
    // `MetricCalibration::classify` continues to mean "worst →
    // Critical". The anchored floors (≤ 5 % coverage Critical, > 75 %
    // Ok) ride alongside the percentiles so calibrated projects keep
    // the literature minimum even when their codebase distribution is
    // uniformly bad.
    let coverage_pct_floors = MetricFloors {
        critical: Some(95.0),
        ok: Some(25.0),
    };
    let coverage_pct = reports.coverage.as_ref().and_then(|c| {
        let values: Vec<f64> = c
            .entries
            .iter()
            .filter(|e| file_filter(&e.path))
            .map(|e| 100.0 - e.line_coverage_pct)
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, coverage_pct_floors))
    });

    // skip_ratio: a percentage with literature anchors > 1% Medium /
    // > 5% High / > 20% Critical (TODO §[features.test]). The codebase
    // distribution refines the percentile breaks above the floor.
    let skip_ratio_floors = MetricFloors {
        critical: Some(20.0),
        ok: Some(0.5),
    };
    let skip_ratio = reports.skip_ratio.as_ref().and_then(|s| {
        let values: Vec<f64> = s
            .entries
            .iter()
            .filter(|e| file_filter(&e.path) && e.skipped_tests > 0)
            .map(|e| e.skip_pct)
            .collect();
        non_empty(&values).then(|| MetricCalibration::from_distribution(&values, skip_ratio_floors))
    });

    // Per-family hotspots: same `HotspotCalibration` shape as code
    // hotspot, anchored on `FLOOR_OK_TEST_HOTSPOT` /
    // `FLOOR_OK_DOC_HOTSPOT`. The floor is intentionally low — high
    // floors silently block legitimate hot files from drain queues,
    // whereas low floors only over-decorate in tiny projects, which
    // calibration percentiles still gate.
    let test_hotspot = reports.test_hotspot.as_ref().and_then(|h| {
        let scores: Vec<f64> = h
            .entries
            .iter()
            .filter(|e| file_filter(&e.path))
            .map(|e| e.score)
            .collect();
        non_empty(&scores).then(|| {
            HotspotCalibration::from_distribution_with_floor(&scores, Some(FLOOR_OK_TEST_HOTSPOT))
        })
    });
    let doc_hotspot = reports.doc_hotspot.as_ref().and_then(|h| {
        // Pair entries are filtered by *doc-side* path so cross-cohort
        // pairs (workspace boundary spans) follow the doc into the
        // workspace it belongs to.
        let scores: Vec<f64> = h
            .entries
            .iter()
            .filter(|e| file_filter(&e.doc_path))
            .map(|e| e.score)
            .collect();
        non_empty(&scores).then(|| {
            HotspotCalibration::from_distribution_with_floor(&scores, Some(FLOOR_OK_DOC_HOTSPOT))
        })
    });

    MetricCalibrations {
        ccn,
        cognitive,
        duplication,
        change_coupling,
        hotspot,
        lcom,
        coverage_pct,
        skip_ratio,
        test_hotspot,
        doc_hotspot,
    }
}

fn has_any_table(m: &MetricCalibrations) -> bool {
    m.ccn.is_some()
        || m.cognitive.is_some()
        || m.duplication.is_some()
        || m.change_coupling.is_some()
        || m.hotspot.is_some()
        || m.lcom.is_some()
        || m.coverage_pct.is_some()
        || m.skip_ratio.is_some()
        || m.test_hotspot.is_some()
        || m.doc_hotspot.is_some()
}

/// Compose every enabled Feature's lowered Findings into one Vec,
/// with Severity and the per-file hotspot flag attached. Thin wrapper
/// around [`crate::feature::FeatureRegistry::lower_all`] — the
/// per-metric branches that used to live inline have moved into
/// per-Feature `lower()` impls under `crate::observer::*`.
///
/// `cfg` drives per-Feature `enabled` checks. Calibration entries that
/// are missing (`cal.calibration.<m>` is `None`) fall back to
/// `Severity::Ok`; the `hotspot` flag is `false` whenever the project
/// has no hotspot calibration.
pub(crate) fn classify(reports: &ObserverReports, cal: &Calibration, cfg: &Config) -> Vec<Finding> {
    let mut findings = crate::feature::FeatureRegistry::builtin().lower_all(reports, cfg, cal);
    let workspaces = &cfg.project.workspaces;
    if !workspaces.is_empty() {
        for f in &mut findings {
            f.workspace = crate::core::config::assign_workspace(&f.location.file, workspaces)
                .map(str::to_owned);
        }
    }
    findings
}

/// Run every observer over `scan_root`, classify against the
/// calibration on disk at `paths`, and pack the result into a fresh
/// `FindingsRecord`. Does not write anything — callers decide whether to
/// persist the result via `findings_cache::write_record`.
///
/// Used by both `heal status` (`scan_root` = project, sha/clean from
/// git) and `heal diff` (`scan_root` = transient `git worktree`, sha
/// = the requested ref, clean = true by construction).
pub(crate) fn build_record(
    scan_root: &Path,
    paths: &crate::core::HealPaths,
    cfg: &Config,
    head_sha: Option<String>,
    worktree_clean: bool,
) -> crate::core::findings_cache::FindingsRecord {
    let calibration = Calibration::load(&paths.calibration())
        .ok()
        .map(|c| c.with_overrides(cfg));
    let owned;
    let cal_ref = if let Some(c) = calibration.as_ref() {
        c
    } else {
        owned = Calibration::default();
        &owned
    };
    let reports = run_all(scan_root, cfg, None, None);
    let findings = classify(&reports, cal_ref, cfg);
    let config_hash =
        crate::core::findings_cache::config_hash_from_paths(&paths.config(), &paths.calibration());
    crate::core::findings_cache::FindingsRecord::new(
        head_sha,
        worktree_clean,
        config_hash,
        findings,
    )
}

fn non_empty(values: &[f64]) -> bool {
    !values.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::calibration::{CalibrationMeta, MetricCalibrations, STRATEGY_PERCENTILE};
    use crate::core::finding::IntoFindings;
    use crate::core::severity::Severity;
    use crate::core::severity::SeverityCounts;
    use crate::observer::code::change_coupling::{ChangeCouplingReport, CouplingTotals, FilePair};
    use crate::observer::code::complexity::{
        ComplexityReport, ComplexityTotals, FileComplexity, FunctionMetric,
    };
    use crate::observer::code::duplication::{
        DuplicateBlock, DuplicateLocation, DuplicationReport, DuplicationTotals, FileDuplication,
    };
    use crate::observer::code::hotspot::{HotspotEntry, HotspotReport, HotspotTotals};
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// Synthesize a small but representative `ObserverReports` +
    /// `Calibration` pair. The numbers are picked so each metric has at
    /// least one non-Ok finding under the calibration's breaks.
    #[allow(clippy::too_many_lines)] // shared test fixture; readability > brevity
    fn fixture() -> (ObserverReports, Calibration) {
        let complexity = ComplexityReport {
            files: vec![FileComplexity {
                path: PathBuf::from("src/hot.rs"),
                language: "rust".into(),
                functions: vec![
                    FunctionMetric {
                        name: "tangled".into(),
                        start_line: 10,
                        end_line: 80,
                        ccn: 30,
                        cognitive: 60,
                    },
                    FunctionMetric {
                        name: "tidy".into(),
                        start_line: 100,
                        end_line: 110,
                        ccn: 2,
                        cognitive: 1,
                    },
                ],
            }],
            totals: ComplexityTotals {
                files: 1,
                functions: 2,
                max_ccn: 30,
                max_cognitive: 60,
            },
        };
        let duplication = DuplicationReport {
            blocks: vec![DuplicateBlock {
                token_count: 80,
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("src/hot.rs"),
                        start_line: 200,
                        end_line: 220,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("src/cold.rs"),
                        start_line: 5,
                        end_line: 25,
                    },
                ],
            }],
            files: vec![
                FileDuplication {
                    path: PathBuf::from("src/hot.rs"),
                    total_tokens: 200,
                    duplicate_tokens: 80,
                    duplicate_pct: 40.0,
                },
                FileDuplication {
                    path: PathBuf::from("src/cold.rs"),
                    total_tokens: 200,
                    duplicate_tokens: 10,
                    duplicate_pct: 5.0,
                },
            ],
            totals: DuplicationTotals {
                duplicate_blocks: 1,
                duplicate_tokens: 90,
                files_affected: 2,
            },
            min_tokens: 50,
        };
        let change_coupling = ChangeCouplingReport {
            pairs: vec![FilePair {
                a: PathBuf::from("src/cold.rs"),
                b: PathBuf::from("src/hot.rs"),
                count: 12,
                direction: None,
                class: None,
            }],
            file_sums: Vec::new(),
            totals: CouplingTotals {
                pairs: 1,
                files: 2,
                commits_considered: 50,
            },
            since_days: 90,
            min_coupling: 3,
        };
        let hotspot = HotspotReport {
            entries: vec![
                HotspotEntry {
                    path: PathBuf::from("src/hot.rs"),
                    ccn_sum: 32,
                    churn_commits: 20,
                    score: 640.0,
                },
                HotspotEntry {
                    path: PathBuf::from("src/cold.rs"),
                    ccn_sum: 4,
                    churn_commits: 2,
                    score: 8.0,
                },
            ],
            totals: HotspotTotals {
                files: 2,
                max_score: 640.0,
            },
        };

        let reports = ObserverReports {
            complexity,
            change_coupling: Some(change_coupling),
            duplication: Some(duplication),
            hotspot: Some(hotspot),
            ..ObserverReports::default()
        };

        let cal = Calibration {
            meta: CalibrationMeta {
                created_at: chrono::Utc::now(),
                codebase_files: 2,
                strategy: STRATEGY_PERCENTILE.to_owned(),
                calibrated_at_sha: None,
            },
            calibration: MetricCalibrations {
                ccn: Some(MetricCalibration {
                    p50: 1.0,
                    p75: 5.0,
                    p90: 10.0,
                    p95: 20.0,
                    floor_critical: Some(FLOOR_CCN),
                    floor_ok: Some(FLOOR_OK_CCN),
                }),
                cognitive: Some(MetricCalibration {
                    p50: 1.0,
                    p75: 10.0,
                    p90: 30.0,
                    p95: 50.0,
                    floor_critical: Some(FLOOR_COGNITIVE),
                    floor_ok: Some(FLOOR_OK_COGNITIVE),
                }),
                duplication: Some(MetricCalibration {
                    p50: 5.0,
                    p75: 10.0,
                    p90: 20.0,
                    p95: 35.0,
                    floor_critical: Some(FLOOR_DUPLICATION_PCT),
                    floor_ok: None,
                }),
                change_coupling: Some(MetricCalibration {
                    p50: 1.0,
                    p75: 4.0,
                    p90: 8.0,
                    p95: 16.0,
                    floor_critical: None,
                    floor_ok: None,
                }),
                hotspot: Some(HotspotCalibration {
                    p50: 8.0,
                    p75: 20.0,
                    p90: 100.0,
                    p95: 500.0,
                    floor_ok: Some(crate::core::calibration::FLOOR_OK_HOTSPOT),
                }),
                lcom: None,
                coverage_pct: None,
                skip_ratio: None,
                test_hotspot: None,
                doc_hotspot: None,
            },
            workspaces: BTreeMap::new(),
        };

        (reports, cal)
    }

    /// Drift detector — `classify` must produce the **same** id set as
    /// the observers' own `IntoFindings::into_findings()` output. If
    /// this ever fails, the cache layer's `Finding.id` matching breaks
    /// silently across `heal status` runs.
    #[test]
    fn classify_id_set_matches_into_findings() {
        let (reports, cal) = fixture();

        let mut want: HashSet<String> = reports
            .complexity
            .into_findings()
            .into_iter()
            .map(|f| f.id)
            .collect();
        if let Some(d) = reports.duplication.as_ref() {
            want.extend(d.into_findings().into_iter().map(|f| f.id));
        }
        if let Some(c) = reports.change_coupling.as_ref() {
            want.extend(c.into_findings().into_iter().map(|f| f.id));
        }
        if let Some(h) = reports.hotspot.as_ref() {
            want.extend(h.into_findings().into_iter().map(|f| f.id));
        }

        let got: HashSet<String> = classify(&reports, &cal, &Config::default())
            .into_iter()
            .map(|f| f.id)
            .collect();

        assert_eq!(
            got, want,
            "classify must produce the same Finding.id set as IntoFindings",
        );
    }

    #[test]
    fn classify_assigns_severity_per_metric() {
        let (reports, cal) = fixture();
        let findings = classify(&reports, &cal, &Config::default());

        let by_metric_severity = |metric: &str, severity: Severity| {
            findings
                .iter()
                .filter(|f| f.metric == metric && f.severity == severity)
                .count()
        };

        // CCN=30 hits floor_critical (25); CCN=2 is below p75 (5) → Ok.
        assert!(by_metric_severity("ccn", Severity::Critical) >= 1);
        assert!(by_metric_severity("ccn", Severity::Ok) >= 1);
        // Cognitive=60 hits floor (50); Cognitive=1 → Ok.
        assert!(by_metric_severity("cognitive", Severity::Critical) >= 1);
        assert!(by_metric_severity("cognitive", Severity::Ok) >= 1);
        // Duplication block max_pct = 40.0 ≥ floor (30) → Critical.
        assert!(by_metric_severity("duplication", Severity::Critical) >= 1);
        // Coupling pair count=12 ≥ p90 (8) but < p95 (16) → High.
        assert!(by_metric_severity("change_coupling", Severity::High) >= 1);
        // Hotspot Findings carry no Severity (always Ok).
        assert!(findings
            .iter()
            .filter(|f| f.metric == "hotspot")
            .all(|f| f.severity == Severity::Ok));
    }

    #[test]
    fn classify_flags_hotspot_for_files_above_p90() {
        let (reports, cal) = fixture();
        let findings = classify(&reports, &cal, &Config::default());

        // src/hot.rs score=640 ≥ p90=100 → hotspot. src/cold.rs score=8 < p90.
        let hot_path = PathBuf::from("src/hot.rs");
        let cold_path = PathBuf::from("src/cold.rs");

        // Every CCN/Cognitive finding on hot.rs must carry hotspot=true.
        assert!(findings
            .iter()
            .filter(
                |f| matches!(f.metric.as_str(), "ccn" | "cognitive") && f.location.file == hot_path
            )
            .all(|f| f.hotspot));

        // The duplication block spans hot.rs + cold.rs — even with
        // primary on hot.rs, the per-locations check should flip
        // hotspot=true.
        assert!(findings
            .iter()
            .filter(|f| f.metric == "duplication")
            .all(|f| f.hotspot));

        // The coupling pair touches hot.rs via `locations`. Pair primary
        // is `cold.rs` (lex-smaller) — verify the secondary check fires.
        let pair = findings
            .iter()
            .find(|f| f.metric == "change_coupling")
            .expect("coupling finding present");
        assert_eq!(pair.location.file, cold_path);
        assert!(
            pair.hotspot,
            "coupling hotspot should consider partner file"
        );
    }

    #[test]
    fn classify_with_missing_calibration_falls_back_to_ok() {
        let (reports, _) = fixture();
        let bare = Calibration {
            meta: CalibrationMeta {
                created_at: chrono::Utc::now(),
                codebase_files: 0,
                strategy: STRATEGY_PERCENTILE.to_owned(),
                calibrated_at_sha: None,
            },
            calibration: MetricCalibrations::default(),
            workspaces: BTreeMap::new(),
        };
        let findings = classify(&reports, &bare, &Config::default());
        assert!(
            findings.iter().all(|f| f.severity == Severity::Ok),
            "without calibration, every Finding must be Severity::Ok",
        );
        assert!(
            findings.iter().all(|f| !f.hotspot),
            "without hotspot calibration, the flag must stay false",
        );
    }

    #[test]
    fn severity_counts_from_findings_matches_classify_count() {
        let (reports, cal) = fixture();
        let findings = classify(&reports, &cal, &Config::default());
        let counts = SeverityCounts::from_findings(&findings);

        let total_classified = counts.critical + counts.high + counts.medium + counts.ok;
        assert_eq!(
            total_classified as usize,
            findings.len(),
            "tally must equal Finding count produced by classify",
        );
    }
}
