//! `heal check` — render `.heal/checks/latest.json` and, when needed,
//! produce it.
//!
//! Default flow reads the cached `CheckRecord` from `latest.json` if
//! one exists. Only when the cache is missing — or `--refresh` is
//! passed — does this command run every observer, lift the reports
//! through `crate::core::finding::IntoFindings`, decorate each Finding
//! with Severity (via `Calibration`) and the per-file hotspot flag
//! (via `HotspotCalibration`), and write a fresh `CheckRecord`. This
//! is still the single writer of `.heal/checks/`.
//!
//! The renderer groups findings under `Critical 🔥 / Critical / High 🔥
//! / High / Medium 🔥 / Medium / Ok 🔥 / Ok` (last four require
//! `--all`), aggregates one row per file, and surfaces a "next steps"
//! footer pointing at `heal check --severity critical` and `claude
//! /heal-code-fix`.
//!
//! `--json` emits the `CheckRecord` in the exact shape of `latest.json`
//! so skills and CI scripts have one stable contract.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::{CheckArgs, CheckMetric, SeverityFilter};
use crate::core::calibration::Calibration;
use crate::core::check_cache::{
    config_hash_from_paths, read_latest, reconcile_fixed, write_record, CheckRecord, RegressedEntry,
};
use crate::core::config::load_from_project;
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::core::snapshot::{ansi_wrap, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observer::git;
use crate::observers::{classify, run_all};

pub fn run(project: &Path, args: &CheckArgs) -> Result<()> {
    let paths = HealPaths::new(project);
    paths.ensure().with_context(|| {
        format!(
            "creating {} (heal-cli needs a writable .heal/ directory)",
            paths.root().display(),
        )
    })?;

    let filters = Filters::from_args(args);

    // Default: reuse the cache if present. `--refresh` forces a fresh
    // scan and overwrite, so skip the cache read entirely in that
    // mode. A missing cache also triggers a scan so the first
    // invocation in a freshly-initialised project still works.
    let cached = if args.refresh {
        None
    } else {
        read_latest(&paths.checks_latest()).ok().flatten()
    };
    let must_scan = cached.is_none();

    let (record, regressed) = if must_scan {
        let cfg = load_from_project(project).with_context(|| {
            format!(
                "loading {} (run `heal init` first?)",
                paths.config().display(),
            )
        })?;
        let calibration = Calibration::load(&paths.calibration())
            .ok()
            .map(|c| c.with_overrides(&cfg));

        let head_sha = git::head_sha(project);
        let worktree_clean = git::worktree_clean(project).unwrap_or(false);
        let config_hash = config_hash_from_paths(&paths.config(), &paths.calibration());

        let record = build_fresh_record(
            project,
            &cfg,
            calibration.as_ref(),
            head_sha,
            worktree_clean,
            config_hash,
        );
        write_record(&paths.checks_dir(), &paths.checks_latest(), &record)?;
        let regs = reconcile_fixed(
            &paths.checks_fixed_log(),
            &paths.checks_regressed_log(),
            &record,
        )?;
        (record, regs)
    } else {
        // Cache hit: skip the scan entirely. Reconciliation already
        // happened on the run that produced this record.
        (
            cached.expect("cache present per must_scan branch"),
            Vec::new(),
        )
    };

    if args.json {
        emit_json(&record);
        return Ok(());
    }
    let mut stdout = std::io::stdout();
    let colorize = stdout.is_terminal();
    render(&record, &regressed, &filters, colorize, &mut stdout)?;
    Ok(())
}

/// Run every observer + classify, returning a fresh `CheckRecord`
/// without writing it. Reused by `heal fix diff` (in "vs live" mode,
/// when no explicit TO is supplied) so a half-finished session can
/// compare against a cached record without polluting `.heal/checks/`.
pub(super) fn build_fresh_record(
    project: &Path,
    cfg: &crate::core::config::Config,
    calibration: Option<&Calibration>,
    head_sha: Option<String>,
    worktree_clean: bool,
    config_hash: String,
) -> CheckRecord {
    let reports = run_all(project, cfg, None);
    let owned;
    let cal_ref = if let Some(c) = calibration {
        c
    } else {
        owned = Calibration::default();
        &owned
    };
    let findings = classify(&reports, cal_ref, cfg);
    CheckRecord::new(head_sha, worktree_clean, config_hash, findings)
}

fn emit_json(record: &CheckRecord) {
    let body =
        serde_json::to_string_pretty(record).expect("CheckRecord serialization is infallible");
    println!("{body}");
}

/// Resolved filters for the renderer.
#[derive(Debug, Clone, Default)]
pub(super) struct Filters {
    pub(super) metric: Option<CheckMetric>,
    pub(super) feature: Option<String>,
    pub(super) severity: Option<Severity>,
    pub(super) all: bool,
    pub(super) top: Option<usize>,
}

impl Filters {
    fn from_args(args: &CheckArgs) -> Self {
        Self {
            metric: args.metric,
            feature: args.feature.clone(),
            severity: args.severity.map(SeverityFilter::into_severity),
            all: args.all,
            top: args.top,
        }
    }

    fn passes(&self, finding: &Finding) -> bool {
        if let Some(m) = self.metric {
            if !m.matches(&finding.metric) {
                return false;
            }
        }
        if let Some(prefix) = self.feature.as_ref() {
            if !finding
                .location
                .file
                .to_string_lossy()
                .starts_with(prefix.as_str())
            {
                return false;
            }
        }
        true
    }
}

/// Render the record to `out`. Pure function — no IO besides writing
/// `out`. Tests pin the prefixes for the section headers; the precise
/// row formatting can evolve without breaking the contract.
#[allow(clippy::too_many_lines)] // bucket-by-bucket pour; splitting hurts readability
pub(super) fn render(
    record: &CheckRecord,
    regressed: &[RegressedEntry],
    filters: &Filters,
    colorize: bool,
    out: &mut impl Write,
) -> Result<()> {
    let title = ansi_wrap(ANSI_CYAN, "── HEAL check", colorize);
    let bar: String = "─".repeat(50);
    writeln!(out, "{title} {bar}")?;
    writeln!(
        out,
        "  Calibrated: {}  ({} findings, head {})",
        record.started_at.format("%Y-%m-%d %H:%M"),
        record.findings.len(),
        record.head_sha.as_deref().unwrap_or("∅"),
    )?;
    if !record.worktree_clean {
        writeln!(
            out,
            "  {} worktree dirty — uncommitted changes are reflected here.",
            ansi_wrap(ANSI_YELLOW, "note:", colorize),
        )?;
    }
    writeln!(out)?;

    if !regressed.is_empty() {
        writeln!(
            out,
            "  {} {} previously-fixed finding(s) re-detected. See `.heal/checks/regressed.jsonl`.",
            ansi_wrap(ANSI_YELLOW, "regression:", colorize),
            regressed.len(),
        )?;
        writeln!(out)?;
    }

    let filtered: Vec<&Finding> = record
        .findings
        .iter()
        .filter(|f| filters.passes(f))
        .collect();

    let show_low = filters.all || matches!(filters.severity, Some(Severity::Medium | Severity::Ok));

    if show_low {
        // Surface the "low-Severity, top-10% hotspot" findings as a
        // pre-bucket section: files HEAL hasn't classified as a problem
        // but that get touched often enough to warrant a look. Folded
        // in here under `--all` (and any explicit Medium/Ok severity
        // floor); the dedicated `--hotspot` flag was retired.
        let hotspots: Vec<&Finding> = filtered
            .iter()
            .filter(|f| f.hotspot && matches!(f.severity, Severity::Ok | Severity::Medium))
            .copied()
            .collect();
        if !hotspots.is_empty() {
            render_section(
                "Ok / Medium 🔥 (low Severity, top-10% hotspot)",
                ANSI_CYAN,
                &hotspots,
                filters.top,
                colorize,
                out,
            )?;
            writeln!(out)?;
        }
    }

    let mut by_severity: BTreeMap<SeverityKey, Vec<&Finding>> = BTreeMap::new();
    for f in &filtered {
        if let Some(min) = filters.severity {
            if f.severity < min {
                continue;
            }
        }
        by_severity
            .entry(SeverityKey::new(f.severity, f.hotspot))
            .or_default()
            .push(f);
    }

    let buckets: &[(SeverityKey, &str, &str)] = &[
        (
            SeverityKey {
                severity: Severity::Critical,
                hotspot: true,
            },
            "🔴 Critical 🔥",
            ANSI_RED,
        ),
        (
            SeverityKey {
                severity: Severity::Critical,
                hotspot: false,
            },
            "🔴 Critical",
            ANSI_RED,
        ),
        (
            SeverityKey {
                severity: Severity::High,
                hotspot: true,
            },
            "🟠 High 🔥",
            ANSI_YELLOW,
        ),
        (
            SeverityKey {
                severity: Severity::High,
                hotspot: false,
            },
            "🟠 High",
            ANSI_YELLOW,
        ),
        (
            SeverityKey {
                severity: Severity::Medium,
                hotspot: true,
            },
            "🟡 Medium 🔥",
            ANSI_YELLOW,
        ),
        (
            SeverityKey {
                severity: Severity::Medium,
                hotspot: false,
            },
            "🟡 Medium",
            ANSI_YELLOW,
        ),
    ];

    for (key, label, color) in buckets {
        if matches!(key.severity, Severity::Medium) && !show_low {
            continue;
        }
        if let Some(items) = by_severity.get(key) {
            render_section(label, color, items, filters.top, colorize, out)?;
        }
    }

    if show_low {
        let oks: Vec<&Finding> = filtered
            .iter()
            .filter(|f| f.severity == Severity::Ok)
            .copied()
            .collect();
        if !oks.is_empty() {
            render_section("✅ Ok", ANSI_GREEN, &oks, filters.top, colorize, out)?;
        }
    } else {
        let hidden = filtered
            .iter()
            .filter(|f| matches!(f.severity, Severity::Medium | Severity::Ok))
            .count();
        if hidden > 0 {
            writeln!(
                out,
                "  ✅ Medium / Ok ({hidden} findings)  [pass --all to show]"
            )?;
        }
    }

    writeln!(out)?;
    let goal = format!(
        "Critical={}  High={}",
        record.severity_counts.critical, record.severity_counts.high,
    );
    writeln!(out, "  Goal: 0 Critical, 0 High  (current: {goal})")?;
    writeln!(
        out,
        "  Next: `heal check --severity critical` / `claude /heal-code-fix`",
    )?;
    let close: String = "─".repeat(60);
    writeln!(out, "{close}")?;
    Ok(())
}

fn render_section(
    label: &str,
    color: &str,
    items: &[&Finding],
    top: Option<usize>,
    colorize: bool,
    out: &mut impl Write,
) -> std::io::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let mut sorted: Vec<&Finding> = items.to_vec();
    // Deterministic order so two runs on the same Findings render
    // byte-for-byte identical output (snapshot-style tests rely on it).
    sorted.sort_by(|a, b| {
        b.hotspot
            .cmp(&a.hotspot)
            .then_with(|| a.metric.cmp(&b.metric))
            .then_with(|| a.location.file.cmp(&b.location.file))
    });
    let total = sorted.len();
    if let Some(n) = top {
        sorted.truncate(n);
    }
    writeln!(out, "{} ({})", ansi_wrap(color, label, colorize), total)?;
    // File-level aggregation: one row per file, joining the per-finding
    // summaries. Most users care about "which file is on fire", not
    // "which symbol within it" — file rows match the TODO mock and
    // the per-finding detail still ships in --json.
    let mut by_file: BTreeMap<&PathBuf, Vec<&Finding>> = BTreeMap::new();
    for f in &sorted {
        by_file.entry(&f.location.file).or_default().push(f);
    }
    for (file, fs) in &by_file {
        let summary = fs
            .iter()
            .map(|f| f.short_label())
            .collect::<Vec<_>>()
            .join("  ");
        writeln!(out, "  {}  {summary}", file.display())?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SeverityKey {
    severity: Severity,
    hotspot: bool,
}

impl SeverityKey {
    fn new(severity: Severity, hotspot: bool) -> Self {
        Self { severity, hotspot }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::finding::Location;
    use std::path::PathBuf;

    fn finding(metric: &str, file: &str, severity: Severity, hotspot: bool) -> Finding {
        let mut f = Finding::new(
            metric,
            Location {
                file: PathBuf::from(file),
                line: Some(1),
                symbol: Some("fn_name".into()),
            },
            format!(
                "{} 42 fn_name (rust)",
                if metric == "ccn" {
                    "CCN="
                } else if metric == "cognitive" {
                    "Cognitive="
                } else {
                    metric
                }
            ),
            metric,
        );
        f.severity = severity;
        f.hotspot = hotspot;
        f
    }

    fn record(findings: Vec<Finding>) -> CheckRecord {
        CheckRecord::new(Some("abc1234".into()), true, "h".into(), findings)
    }

    fn render_to_string(record: &CheckRecord, filters: &Filters) -> String {
        let mut buf = Vec::new();
        render(record, &[], filters, false, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn default_filters() -> Filters {
        Filters {
            metric: None,
            feature: None,
            severity: None,
            all: false,
            top: None,
        }
    }

    #[test]
    fn renders_critical_hotspot_above_critical_plain() {
        let rec = record(vec![
            finding("ccn", "src/cool.ts", Severity::Critical, false),
            finding("ccn", "src/hot.ts", Severity::Critical, true),
        ]);
        let out = render_to_string(&rec, &default_filters());
        let hot_idx = out.find("Critical 🔥").expect("hot section exists");
        let plain_idx = out
            .find("\nCritical (")
            .or_else(|| out.find(" Critical ("))
            .expect("plain critical section exists");
        // The hot section must precede the plain section.
        assert!(
            hot_idx < plain_idx,
            "Critical 🔥 must render before Critical:\n{out}",
        );
    }

    #[test]
    fn hides_medium_and_ok_without_all_flag() {
        let rec = record(vec![
            finding("ccn", "src/hot.ts", Severity::Critical, false),
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false),
            finding("ccn", "src/cold.ts", Severity::Ok, false),
        ]);
        let out = render_to_string(&rec, &default_filters());
        assert!(out.contains("Critical"), "should show Critical");
        assert!(
            !out.contains("🟡 Medium"),
            "Medium section should be hidden by default"
        );
        assert!(out.contains("Medium / Ok (2 findings)"));
    }

    #[test]
    fn all_flag_shows_medium_and_ok() {
        let rec = record(vec![
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false),
            finding("ccn", "src/cold.ts", Severity::Ok, false),
        ]);
        let mut filters = default_filters();
        filters.all = true;
        let out = render_to_string(&rec, &filters);
        assert!(out.contains("Medium"), "Medium must render with --all");
        assert!(out.contains("Ok"), "Ok must render with --all");
    }

    #[test]
    fn metric_filter_drops_other_metrics() {
        let rec = record(vec![
            finding("ccn", "src/a.ts", Severity::Critical, false),
            finding("duplication", "src/b.ts", Severity::Critical, false),
        ]);
        let mut filters = default_filters();
        filters.metric = Some(CheckMetric::Ccn);
        let out = render_to_string(&rec, &filters);
        assert!(out.contains("src/a.ts"));
        assert!(!out.contains("src/b.ts"));
    }

    #[test]
    fn feature_filter_keeps_path_prefix_only() {
        let rec = record(vec![
            finding("ccn", "src/payments/engine.ts", Severity::Critical, false),
            finding("ccn", "src/billing/cart.ts", Severity::Critical, false),
        ]);
        let mut filters = default_filters();
        filters.feature = Some("src/payments".to_owned());
        let out = render_to_string(&rec, &filters);
        assert!(out.contains("src/payments/engine.ts"));
        assert!(!out.contains("src/billing/cart.ts"));
    }

    #[test]
    fn severity_filter_drops_below_floor() {
        let rec = record(vec![
            finding("ccn", "src/hot.ts", Severity::Critical, false),
            finding("ccn", "src/warm.ts", Severity::High, false),
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false),
        ]);
        let mut filters = default_filters();
        filters.severity = Some(Severity::High);
        let out = render_to_string(&rec, &filters);
        assert!(out.contains("src/hot.ts"));
        assert!(out.contains("src/warm.ts"));
        assert!(
            !out.contains("src/lukewarm.ts"),
            "Medium must drop with --severity high"
        );
    }

    #[test]
    fn all_flag_surfaces_low_severity_hotspot_section() {
        let rec = record(vec![
            finding("hotspot", "src/touch_a_lot.ts", Severity::Ok, true),
            finding("hotspot", "src/quiet.ts", Severity::Ok, false),
        ]);
        let mut filters = default_filters();
        filters.all = true;
        let out = render_to_string(&rec, &filters);
        assert!(
            out.contains("Ok / Medium 🔥"),
            "low-Severity hotspot section must appear under --all:\n{out}",
        );
        assert!(
            out.contains("src/touch_a_lot.ts"),
            "Ok-with-hotspot must surface in the section:\n{out}",
        );
        // Quiet (non-hotspot) Ok findings still render in the standard
        // Ok bucket — the section is *additional* context, not a filter.
        assert!(out.contains("src/quiet.ts"));
    }

    #[test]
    fn default_omits_low_severity_hotspot_section() {
        let rec = record(vec![finding(
            "hotspot",
            "src/touch_a_lot.ts",
            Severity::Ok,
            true,
        )]);
        let out = render_to_string(&rec, &default_filters());
        assert!(
            !out.contains("Ok / Medium 🔥"),
            "low-Severity hotspot section must stay hidden without --all:\n{out}",
        );
    }

    #[test]
    fn empty_record_renders_goal_line() {
        let rec = record(Vec::new());
        let out = render_to_string(&rec, &default_filters());
        assert!(out.contains("Goal: 0 Critical, 0 High"));
        assert!(out.contains("Next:"));
    }
}
