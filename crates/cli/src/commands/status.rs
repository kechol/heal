//! `heal status` — render `.heal/checks/latest.json` and, when needed,
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
//! The renderer groups findings into three drain tiers driven by the
//! `[policy.drain]` config: **Drain queue** (T0 / `must`), **Should
//! drain** (T1 / `should`), and **Advisory** (the rest above
//! `Severity::Ok`). Default `must = ["critical:hotspot"]`,
//! `should = ["critical", "high:hotspot"]`. Within each tier rows are
//! sorted by `Severity` 🔥 desc. Advisory + Ok are hidden unless
//! `--all` is passed; the footer surfaces a "next steps" line pointing
//! at `claude /heal-code-patch` for the Drain queue.
//!
//! `--json` emits the `CheckRecord` in the exact shape of `latest.json`
//! so skills and CI scripts have one stable contract.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::{FindingMetric, SeverityFilter, StatusArgs};
use crate::core::calibration::{
    Calibration, FLOOR_CCN, FLOOR_COGNITIVE, FLOOR_OK_CCN, FLOOR_OK_COGNITIVE,
};
use crate::core::check_cache::{
    config_hash_from_paths, read_latest, reconcile_fixed, write_record, CheckRecord, RegressedEntry,
};
use crate::core::config::{load_from_project, Config, DrainTier};
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::core::snapshot::{ansi_wrap, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observer::git;
use crate::observers::{classify, run_all};

pub fn run(project: &Path, args: &StatusArgs) -> Result<()> {
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

    // Load config only when it's actually needed: a fresh scan needs
    // it for build_fresh_record + with_overrides, and the textual
    // renderer needs it for `policy.drain` + override notes. The JSON
    // path with a cache hit pays neither cost.
    let need_cfg = must_scan || !args.json;
    let cfg = if need_cfg {
        Some(load_from_project(project).with_context(|| {
            format!(
                "loading {} (run `heal init` first?)",
                paths.config().display(),
            )
        })?)
    } else {
        None
    };

    let (record, regressed) = if must_scan {
        let cfg = cfg.as_ref().expect("cfg loaded above when must_scan");
        let record = build_live_record(project, &paths, cfg);
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
        match filters.workspace.as_deref() {
            None => super::emit_json(&record),
            Some(ws) => super::emit_json(&record.project_to_workspace(ws)),
        }
        return Ok(());
    }
    let cfg = cfg.expect("cfg loaded above when not args.json");
    let mut stdout = std::io::stdout();
    let colorize = stdout.is_terminal();
    render(&record, &regressed, &filters, &cfg, colorize, &mut stdout)?;
    Ok(())
}

/// Resolve calibration + git state + config hash, then build a fresh
/// `CheckRecord`. The shared "live scan" preamble: both `heal status`
/// (when the cache misses) and `heal diff` (the right-hand side is
/// always live) call this. Does **not** write anything to disk —
/// callers decide whether to persist the result.
pub(super) fn build_live_record(
    project: &Path,
    paths: &HealPaths,
    cfg: &crate::core::config::Config,
) -> CheckRecord {
    let calibration = Calibration::load(&paths.calibration())
        .ok()
        .map(|c| c.with_overrides(cfg));
    let head_sha = git::head_sha(project);
    let worktree_clean = git::worktree_clean(project).unwrap_or(false);
    let config_hash = config_hash_from_paths(&paths.config(), &paths.calibration());
    build_fresh_record(
        project,
        cfg,
        calibration.as_ref(),
        head_sha,
        worktree_clean,
        config_hash,
    )
}

/// Run every observer + classify, returning a fresh `CheckRecord`
/// without writing it. The lower-level primitive that
/// [`build_live_record`] wraps with the calibration / git / config
/// preamble.
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

/// Resolved filters for the renderer.
#[derive(Debug, Clone, Default)]
pub(super) struct Filters {
    pub(super) metric: Option<FindingMetric>,
    pub(super) feature: Option<String>,
    pub(super) workspace: Option<String>,
    pub(super) severity: Option<Severity>,
    pub(super) all: bool,
    pub(super) top: Option<usize>,
}

impl Filters {
    fn from_args(args: &StatusArgs) -> Self {
        Self {
            metric: args.metric,
            feature: args.feature.clone(),
            workspace: args.workspace.clone(),
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
        if let Some(ws) = self.workspace.as_deref() {
            if finding.workspace.as_deref() != Some(ws) {
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
#[allow(clippy::too_many_lines)] // tier-by-tier pour; splitting hurts readability
pub(super) fn render(
    record: &CheckRecord,
    regressed: &[RegressedEntry],
    filters: &Filters,
    cfg: &Config,
    colorize: bool,
    out: &mut impl Write,
) -> Result<()> {
    let drain = &cfg.policy.drain;
    let title = ansi_wrap(ANSI_CYAN, "── HEAL status", colorize);
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
    if let Some(ws) = filters.workspace.as_deref() {
        writeln!(out, "  workspace: {ws}")?;
    }
    for line in override_notes(cfg) {
        writeln!(
            out,
            "  {} {line}",
            ansi_wrap(ANSI_CYAN, "override:", colorize)
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

    let show_low = filters.all || matches!(filters.severity, Some(Severity::Medium | Severity::Ok));

    // Single pass: bucket each finding into its drain tier, the Ok pile,
    // or — when relevant — the Ok 🔥 pre-section pile.
    let mut hotspot_oks: Vec<&Finding> = Vec::new();
    let mut by_tier: BTreeMap<DrainTier, Vec<&Finding>> = BTreeMap::new();
    let mut ok_findings: Vec<&Finding> = Vec::new();
    for f in record.findings.iter().filter(|f| filters.passes(f)) {
        if let Some(min) = filters.severity {
            if f.severity < min {
                continue;
            }
        }
        if f.severity == Severity::Ok {
            if show_low && f.hotspot {
                hotspot_oks.push(f);
            }
            ok_findings.push(f);
            continue;
        }
        if let Some(tier) = drain.tier_for(f) {
            by_tier.entry(tier).or_default().push(f);
        }
    }

    if !hotspot_oks.is_empty() {
        render_tier_section(
            "Ok 🔥 (low Severity, top-10% hotspot)",
            ANSI_CYAN,
            &hotspot_oks,
            filters.top,
            colorize,
            out,
        )?;
        writeln!(out)?;
    }

    let tiers: &[(DrainTier, &str, &str, bool)] = &[
        (DrainTier::Must, "🎯 Drain queue (T0)", ANSI_RED, true),
        (DrainTier::Should, "🟡 Should drain (T1)", ANSI_YELLOW, true),
        // Advisory only renders under --all (or explicit low severity).
        (DrainTier::Advisory, "ℹ️  Advisory", ANSI_CYAN, show_low),
    ];

    let mut hidden_count = 0usize;
    for (tier, label, color, visible) in tiers {
        let Some(items) = by_tier.get(tier) else {
            continue;
        };
        if !*visible {
            hidden_count += items.len();
            continue;
        }
        render_tier_section(label, color, items, filters.top, colorize, out)?;
    }

    if show_low && !ok_findings.is_empty() {
        render_tier_section(
            "✅ Ok",
            ANSI_GREEN,
            &ok_findings,
            filters.top,
            colorize,
            out,
        )?;
    } else if !show_low && (hidden_count > 0 || !ok_findings.is_empty()) {
        let advisory_hidden = hidden_count;
        let ok_hidden = ok_findings.len();
        writeln!(
            out,
            "  Hidden: {advisory_hidden} advisory, {ok_hidden} Ok findings  [pass --all to show]",
        )?;
    }

    writeln!(out)?;
    let must_count = by_tier.get(&DrainTier::Must).map_or(0, Vec::len);
    let should_count = by_tier.get(&DrainTier::Should).map_or(0, Vec::len);
    writeln!(
        out,
        "  Goal: 0 in Drain queue  (T0: {must_count}, T1: {should_count})",
    )?;
    writeln!(
        out,
        "  Next: `claude /heal-code-patch` drains the T0 queue one finding per commit",
    )?;
    let close: String = "─".repeat(60);
    writeln!(out, "{close}")?;
    Ok(())
}

/// Render a drain-tier section with internal Severity 🔥 sort.
fn render_tier_section(
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
    // Severity desc, then 🔥 first within same Severity, then by metric
    // / file for deterministic snapshot output.
    sorted.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| b.hotspot.cmp(&a.hotspot))
            .then_with(|| a.metric.cmp(&b.metric))
            .then_with(|| a.location.file.cmp(&b.location.file))
    });
    let total = sorted.len();
    if let Some(n) = top {
        sorted.truncate(n);
    }
    writeln!(out, "{} ({})", ansi_wrap(color, label, colorize), total)?;
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

/// One-line notes for any per-metric `floor_ok` / `floor_critical`
/// override active in the config. Surfaced near the header so users see
/// "policy moved" without digging through `.heal/config.toml`.
fn override_notes(cfg: &Config) -> Vec<String> {
    let ccn = &cfg.metrics.ccn;
    let cog = &cfg.metrics.cognitive;
    if ccn.floor_ok.is_none()
        && ccn.floor_critical.is_none()
        && cog.floor_ok.is_none()
        && cog.floor_critical.is_none()
    {
        return Vec::new();
    }
    let push =
        |notes: &mut Vec<String>, metric: &str, kind: &str, value: Option<f64>, baseline: f64| {
            if let Some(v) = value {
                if (v - baseline).abs() > f64::EPSILON {
                    notes.push(format!("{metric} {kind}={v} [override from {baseline}]"));
                }
            }
        };
    let mut notes = Vec::with_capacity(4);
    push(&mut notes, "ccn", "floor_ok", ccn.floor_ok, FLOOR_OK_CCN);
    push(
        &mut notes,
        "cognitive",
        "floor_ok",
        cog.floor_ok,
        FLOOR_OK_COGNITIVE,
    );
    push(
        &mut notes,
        "ccn",
        "floor_critical",
        ccn.floor_critical,
        FLOOR_CCN,
    );
    push(
        &mut notes,
        "cognitive",
        "floor_critical",
        cog.floor_critical,
        FLOOR_COGNITIVE,
    );
    notes
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
        let cfg = Config::default();
        render(record, &[], filters, &cfg, false, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn default_filters() -> Filters {
        Filters {
            metric: None,
            feature: None,
            workspace: None,
            severity: None,
            all: false,
            top: None,
        }
    }

    #[test]
    fn drain_queue_renders_before_should_drain() {
        // Critical 🔥 → Drain queue (T0); plain Critical → Should drain (T1).
        // Drain queue must print first so the operator sees must-fix items
        // above bandwidth-permitting items.
        let rec = record(vec![
            finding("ccn", "src/cool.ts", Severity::Critical, false),
            finding("ccn", "src/hot.ts", Severity::Critical, true),
        ]);
        let out = render_to_string(&rec, &default_filters());
        let must_idx = out.find("Drain queue").expect("Drain queue section");
        let should_idx = out.find("Should drain").expect("Should drain section");
        assert!(
            must_idx < should_idx,
            "Drain queue must render before Should drain:\n{out}",
        );
        // Both files appear in their respective tier sections.
        assert!(out.contains("src/hot.ts"));
        assert!(out.contains("src/cool.ts"));
    }

    #[test]
    fn default_hides_advisory_and_ok() {
        let rec = record(vec![
            finding("ccn", "src/hot.ts", Severity::Critical, true), // → T0
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false), // → Advisory
            finding("ccn", "src/cold.ts", Severity::Ok, false),     // → Ok
        ]);
        let out = render_to_string(&rec, &default_filters());
        assert!(out.contains("Drain queue"), "T0 must render");
        assert!(
            !out.contains("Advisory"),
            "Advisory must be hidden by default:\n{out}"
        );
        assert!(
            out.contains("1 advisory, 1 Ok"),
            "Hidden summary must surface counts:\n{out}",
        );
    }

    #[test]
    fn all_flag_shows_advisory_and_ok() {
        let rec = record(vec![
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false), // → Advisory
            finding("ccn", "src/cold.ts", Severity::Ok, false),         // → Ok
        ]);
        let mut filters = default_filters();
        filters.all = true;
        let out = render_to_string(&rec, &filters);
        assert!(
            out.contains("Advisory"),
            "Advisory must render with --all:\n{out}"
        );
        assert!(out.contains("✅ Ok"), "Ok section must render with --all");
    }

    #[test]
    fn metric_filter_drops_other_metrics() {
        let rec = record(vec![
            finding("ccn", "src/a.ts", Severity::Critical, false),
            finding("duplication", "src/b.ts", Severity::Critical, false),
        ]);
        let mut filters = default_filters();
        filters.metric = Some(FindingMetric::Ccn);
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
        // src/warm.ts (High, no hotspot) lands in Advisory under default
        // policy. `--severity high` keeps it (>= High); `--severity` also
        // implies show_low so Advisory becomes visible.
        let rec = record(vec![
            finding("ccn", "src/hot.ts", Severity::Critical, false), // → Should
            finding("ccn", "src/warm.ts", Severity::High, false),    // → Advisory
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false), // → Advisory but filtered out
        ]);
        let mut filters = default_filters();
        filters.severity = Some(Severity::High);
        filters.all = true; // make Advisory visible
        let out = render_to_string(&rec, &filters);
        assert!(out.contains("src/hot.ts"));
        assert!(out.contains("src/warm.ts"));
        assert!(
            !out.contains("src/lukewarm.ts"),
            "Medium must drop with --severity high:\n{out}"
        );
    }

    #[test]
    fn all_flag_surfaces_low_severity_hotspot_section() {
        // Ok 🔥 — touched-a-lot but below floor. Appears in the dedicated
        // pre-section as well as the Ok bucket under --all.
        let rec = record(vec![
            finding("hotspot", "src/touch_a_lot.ts", Severity::Ok, true),
            finding("hotspot", "src/quiet.ts", Severity::Ok, false),
        ]);
        let mut filters = default_filters();
        filters.all = true;
        let out = render_to_string(&rec, &filters);
        assert!(
            out.contains("Ok 🔥"),
            "low-Severity hotspot section must appear under --all:\n{out}",
        );
        assert!(
            out.contains("src/touch_a_lot.ts"),
            "Ok-with-hotspot must surface in the section:\n{out}",
        );
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
            !out.contains("Ok 🔥"),
            "low-Severity hotspot section must stay hidden without --all:\n{out}",
        );
    }

    #[test]
    fn override_notes_surface_in_header() {
        let rec = record(Vec::new());
        let mut cfg = Config::default();
        cfg.metrics.ccn.floor_ok = Some(15.0);
        let mut buf = Vec::new();
        render(&rec, &[], &default_filters(), &cfg, false, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(
            out.contains("ccn floor_ok=15"),
            "override note must surface:\n{out}",
        );
        assert!(out.contains("override from 11"), "{out}");
    }

    #[test]
    fn override_notes_silent_when_at_default() {
        let rec = record(Vec::new());
        let mut cfg = Config::default();
        // Setting to literature default is not an override.
        cfg.metrics.ccn.floor_ok = Some(11.0);
        let mut buf = Vec::new();
        render(&rec, &[], &default_filters(), &cfg, false, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(
            !out.contains("override"),
            "no override note when value matches default:\n{out}"
        );
    }

    #[test]
    fn empty_record_renders_goal_line() {
        let rec = record(Vec::new());
        let out = render_to_string(&rec, &default_filters());
        assert!(
            out.contains("Goal: 0 in Drain queue"),
            "goal line must reference Drain queue:\n{out}",
        );
        assert!(out.contains("Next:"));
        assert!(out.contains("/heal-code-patch"));
    }
}
