//! `heal status` — render `.heal/findings/latest.json` and, when needed,
//! produce it.
//!
//! Default flow reads the cached `FindingsRecord` from `latest.json` if
//! one exists. Only when the cache is missing — or `--refresh` is
//! passed — does this command run every observer, lift the reports
//! through `crate::core::finding::IntoFindings`, decorate each Finding
//! with Severity (via `Calibration`) and the per-file hotspot flag
//! (via `HotspotCalibration`), and write a fresh `FindingsRecord`. This
//! is still the single writer of `.heal/findings/`.
//!
//! The renderer groups findings by `(Severity, hotspot)` and labels the
//! sections by Severity (🔴 Critical 🔥 → 🔴 Critical → 🟠 High 🔥 → …).
//! Each section header carries a `[T0 Must drain]` / `[T1 Should drain]`
//! / `[Advisory]` suffix derived from `[policy.drain]` so the link to
//! `/heal-code-patch` stays explicit. Default policy:
//! `must = ["critical:hotspot"]`, `should = ["critical", "high:hotspot"]`.
//! Sections below `🟠 High 🔥` (plain High, Medium, Ok) are hidden unless
//! `--all` is passed; the footer surfaces a "next steps" line pointing
//! at `claude /heal-code-patch` for the Must-drain queue.
//!
//! `--json` emits the `FindingsRecord` in the exact shape of `latest.json`
//! so skills and CI scripts have one stable contract.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::{FindingMetric, SeverityFilter, StatusArgs};
use crate::core::calibration::{FLOOR_CCN, FLOOR_COGNITIVE, FLOOR_OK_CCN, FLOOR_OK_COGNITIVE};
use crate::core::config::{load_from_project, Config, DrainTier};
use crate::core::finding::Finding;
use crate::core::findings_cache::{
    read_latest, reconcile_fixed, write_record, FindingsRecord, RegressedEntry,
};
use crate::core::severity::Severity;
use crate::core::term::{
    ansi_wrap, write_through_pager, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW,
};
use crate::core::HealPaths;
use crate::observer::git;
use crate::observers::build_record;

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
        read_latest(&paths.findings_latest()).ok().flatten()
    };
    let must_scan = cached.is_none();

    // Load config only when it's actually needed: a fresh scan needs
    // it for `build_record`, and the textual renderer needs it for
    // `policy.drain` + override notes. The JSON path with a cache hit
    // pays neither cost.
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
        let head_sha = git::head_sha(project);
        let worktree_clean = git::worktree_clean(project).unwrap_or(false);
        let record = build_record(project, &paths, cfg, head_sha, worktree_clean);
        write_record(&paths.findings_latest(), &record)?;
        let regs = reconcile_fixed(
            &paths.findings_fixed(),
            &paths.findings_regressed_log(),
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
    write_through_pager(args.no_pager, |out, colorize| {
        render(&record, &regressed, &filters, &cfg, colorize, out)
    })
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
    record: &FindingsRecord,
    regressed: &[RegressedEntry],
    filters: &Filters,
    cfg: &Config,
    colorize: bool,
    out: &mut (impl Write + ?Sized),
) -> Result<()> {
    let drain = &cfg.policy.drain;
    writeln!(
        out,
        "  Calibrated: {}  ({} findings, head {})",
        record.started_at.format("%Y-%m-%d %H:%M"),
        record.findings.len(),
        record.head_sha.as_deref().unwrap_or("∅"),
    )?;
    writeln!(out, "  {}", record.severity_counts.render_inline(colorize))?;
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
            "  {} {} previously-fixed finding(s) re-detected. See `.heal/findings/regressed.jsonl`.",
            ansi_wrap(ANSI_YELLOW, "regression:", colorize),
            regressed.len(),
        )?;
        writeln!(out)?;
    }

    let show_low = filters.all || matches!(filters.severity, Some(Severity::Medium | Severity::Ok));

    // Bucket by (severity, hotspot). Sections are labelled by Severity so
    // the header counts (`[critical] N`) line up with the visible content;
    // the drain-tier (`[T0 Must drain]` etc.) is inferred from
    // `[policy.drain]` and shown as a per-section suffix so the link to
    // `/heal-code-patch` stays explicit without overriding the labels.
    let mut buckets: BTreeMap<(Severity, bool), Vec<&Finding>> = BTreeMap::new();
    for f in record.findings.iter().filter(|f| filters.passes(f)) {
        if let Some(min) = filters.severity {
            if f.severity < min {
                continue;
            }
        }
        buckets.entry((f.severity, f.hotspot)).or_default().push(f);
    }

    // Default-visible sections cover the drain queue + should-drain rows
    // under the literature-anchored policy default. Everything below the
    // Should line is gated behind `--all` (or an explicit low `--severity`).
    let order: &[(Severity, bool, &str, &str, bool)] = &[
        (Severity::Critical, true, "🔴 Critical 🔥", ANSI_RED, true),
        (Severity::Critical, false, "🔴 Critical", ANSI_RED, true),
        (Severity::High, true, "🟠 High 🔥", ANSI_YELLOW, true),
        (Severity::High, false, "🟠 High", ANSI_YELLOW, show_low),
        (
            Severity::Medium,
            true,
            "🟡 Medium 🔥",
            ANSI_YELLOW,
            show_low,
        ),
        (Severity::Medium, false, "🟡 Medium", ANSI_YELLOW, show_low),
        (Severity::Ok, true, "✅ Ok 🔥", ANSI_CYAN, show_low),
        (Severity::Ok, false, "✅ Ok", ANSI_GREEN, show_low),
    ];

    let mut hidden_count = 0usize;
    for (sev, hot, label, color, visible) in order {
        let Some(items) = buckets.get(&(*sev, *hot)) else {
            continue;
        };
        if !*visible {
            hidden_count += items.len();
            continue;
        }
        let suffix = drain
            .tier_for(items[0])
            .map(|t| {
                let name = match t {
                    DrainTier::Must => "T0 Must drain",
                    DrainTier::Should => "T1 Should drain",
                    DrainTier::Advisory => "Advisory",
                };
                format!(" [{name}]")
            })
            .unwrap_or_default();
        let full_label = format!("{label}{suffix}");
        render_tier_section(&full_label, color, items, filters.top, colorize, out)?;
    }

    if !show_low && hidden_count > 0 {
        writeln!(
            out,
            "  Hidden: {hidden_count} findings  [pass --all to show]",
        )?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "  Next: `claude /heal-code-patch` drains the T0 queue one finding per commit",
    )?;
    Ok(())
}

/// Render a drain-tier section with internal Severity 🔥 sort.
fn render_tier_section(
    label: &str,
    color: &str,
    items: &[&Finding],
    top: Option<usize>,
    colorize: bool,
    out: &mut (impl Write + ?Sized),
) -> std::io::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let mut sorted: Vec<&Finding> = items.to_vec();
    // Severity desc, then 🔥 first within same Severity, then by metric
    // / file for deterministic output.
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

    fn record(findings: Vec<Finding>) -> FindingsRecord {
        FindingsRecord::new(Some("abc1234".into()), true, "h".into(), findings)
    }

    fn render_to_string(record: &FindingsRecord, filters: &Filters) -> String {
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
    fn critical_hotspot_renders_before_plain_critical() {
        // Critical 🔥 (T0 Must drain) must print before plain Critical
        // (T1 Should drain) so the operator sees must-fix items first.
        let rec = record(vec![
            finding("ccn", "src/cool.ts", Severity::Critical, false),
            finding("ccn", "src/hot.ts", Severity::Critical, true),
        ]);
        let out = render_to_string(&rec, &default_filters());
        let hot_idx = out.find("Critical 🔥").expect("Critical 🔥 section");
        let plain_idx = {
            // "Critical 🔥" header contains "Critical"; find the second occurrence.
            let after_hot = &out[hot_idx + "Critical 🔥".len()..];
            after_hot
                .find("🔴 Critical ")
                .map(|i| hot_idx + "Critical 🔥".len() + i)
                .or_else(|| {
                    after_hot
                        .find("🔴 Critical")
                        .map(|i| hot_idx + "Critical 🔥".len() + i)
                })
                .expect("plain Critical section")
        };
        assert!(
            hot_idx < plain_idx,
            "Critical 🔥 must render before plain Critical:\n{out}",
        );
        assert!(out.contains("[T0 Must drain]"), "T0 tier suffix:\n{out}");
        assert!(out.contains("[T1 Should drain]"), "T1 tier suffix:\n{out}");
        assert!(out.contains("src/hot.ts"));
        assert!(out.contains("src/cool.ts"));
    }

    #[test]
    fn default_hides_medium_and_ok() {
        let rec = record(vec![
            finding("ccn", "src/hot.ts", Severity::Critical, true), // visible
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false), // hidden
            finding("ccn", "src/cold.ts", Severity::Ok, false),     // hidden
        ]);
        let out = render_to_string(&rec, &default_filters());
        assert!(out.contains("🔴 Critical 🔥"), "Critical 🔥 must render");
        assert!(
            !out.contains("🟡 Medium"),
            "Medium must be hidden by default:\n{out}"
        );
        assert!(
            !out.contains("✅ Ok"),
            "Ok must be hidden by default:\n{out}"
        );
        assert!(
            out.contains("Hidden: 2 findings"),
            "Hidden summary must surface counts:\n{out}",
        );
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
        assert!(out.contains("🟡 Medium"), "Medium must render with --all");
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
        let rec = record(vec![
            finding("ccn", "src/hot.ts", Severity::Critical, false),
            finding("ccn", "src/warm.ts", Severity::High, false),
            finding("ccn", "src/lukewarm.ts", Severity::Medium, false),
        ]);
        let mut filters = default_filters();
        filters.severity = Some(Severity::High);
        filters.all = true; // make low-severity sections visible
        let out = render_to_string(&rec, &filters);
        assert!(out.contains("src/hot.ts"));
        assert!(out.contains("src/warm.ts"));
        assert!(
            !out.contains("src/lukewarm.ts"),
            "Medium must drop with --severity high:\n{out}"
        );
    }

    #[test]
    fn ok_hotspot_renders_after_drain_sections_under_all() {
        // Ok 🔥 — touched-a-lot but below floor. Must render below
        // Critical / High sections so the drain queue stays at the top.
        let rec = record(vec![
            finding("ccn", "src/hot.ts", Severity::Critical, true),
            finding("hotspot", "src/touch_a_lot.ts", Severity::Ok, true),
        ]);
        let mut filters = default_filters();
        filters.all = true;
        let out = render_to_string(&rec, &filters);
        let critical_idx = out.find("🔴 Critical 🔥").expect("Critical 🔥 section");
        let ok_hot_idx = out.find("✅ Ok 🔥").expect("Ok 🔥 section");
        assert!(
            critical_idx < ok_hot_idx,
            "Drain queue (Critical 🔥) must render above Ok 🔥:\n{out}",
        );
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
    fn empty_record_renders_next_hint() {
        let rec = record(Vec::new());
        let out = render_to_string(&rec, &default_filters());
        assert!(out.contains("Next:"));
        assert!(out.contains("/heal-code-patch"));
        assert!(
            !out.contains("── HEAL status"),
            "leading divider should be removed:\n{out}",
        );
        assert!(
            !out.contains("Goal:"),
            "trailing Goal line should be removed:\n{out}",
        );
    }
}
