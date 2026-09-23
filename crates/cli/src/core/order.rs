//! Drain-queue order: the single definition shared by `heal status`, the
//! patch skills (through `heal status --json`), and the drain-order backtest.
//!
//! Tier, then Severity, then the family-local `hotspot_score`, then
//! deterministic metric / path / id ties. When `[features.semantic]`
//! verdicts are present, their ordering axes slot in between Severity
//! and `hotspot_score` (see [`within_severity`]), compared one after
//! another rather than combined into a score; they never change a
//! Finding's Tier or Severity, so `design-philosophy.md` §1.2 (no single
//! composite score) and §1.3 (Severity and Hotspot stay orthogonal) hold.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::core::config::PolicyDrainConfig;
use crate::core::finding::Finding;

fn hotspot_desc(a: &Finding, b: &Finding) -> Ordering {
    match (b.hotspot_score, a.hotspot_score) {
        (Some(b), Some(a)) => b.total_cmp(&a),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

/// Notes below this confidence do not move a Finding.
const MIN_CONFIDENCE: f64 = 0.5;

fn noted<'a>(f: &'a Finding, name: &str) -> Option<&'a crate::core::finding::SemanticNote> {
    f.semantic
        .get(name)
        .filter(|n| n.confidence >= MIN_CONFIDENCE)
}

/// One bucketed value per semantic axis, in comparison order. Higher
/// sorts first. Each axis is coarse on purpose so a later axis still
/// separates Findings an earlier one ties; a Finding without a note sits
/// at the axis's neutral middle, so a teammate without verdicts (or a
/// project without `[features.semantic]`) gets exactly the plain
/// Tier → Severity → `hotspot_score` order.
///
/// 1. `focus` — how much the `--focus` work touches the file (0–3; 0 without a note).
/// 2. `consequence` — dev (0) … critical (3); neutral 1.5.
/// 3. `friction.*` — any of change / test / read ≥ 0.7 → 2; all < 0.3 → 0; neutral 1.
/// 4. `fix_ratio` — share of recent commits that were bug fixes, in quarters.
/// 5. `effort` — local first (2), contained (1), cross-file (0); neutral 1.
fn semantic_axes(f: &Finding) -> [i32; 5] {
    let level = |name: &str, labels: &[&str], neutral: i32| {
        noted(f, name).map_or(neutral, |n| {
            labels
                .iter()
                .position(|l| *l == n.label)
                .map_or(neutral, |i| i32::try_from(i * 2).unwrap_or(neutral))
        })
    };
    let focus = level("focus", &["none", "read", "touch", "change"], 0);
    let consequence = level(
        "consequence",
        &["dev", "internal", "user_facing", "critical"],
        3,
    );
    let frictions: Vec<f64> = ["friction.change", "friction.test", "friction.read"]
        .iter()
        .filter_map(|k| noted(f, k).map(|n| n.p))
        .collect();
    let friction = if frictions.is_empty() {
        1
    } else if frictions.iter().any(|p| *p >= 0.7) {
        2
    } else {
        i32::from(!frictions.iter().all(|p| *p < 0.3))
    };
    #[allow(clippy::cast_possible_truncation)]
    let fix_ratio = f
        .semantic
        .get("fix_ratio")
        .map_or(0, |n| (n.p.clamp(0.0, 1.0) * 4.0).round() as i32);
    let effort = level("effort", &["cross_file", "contained", "local"], 1);
    [focus, consequence, friction, fix_ratio, effort]
}

/// Order of two Findings that already share Tier and Severity: the
/// semantic axes (when present), then `hotspot_score`, then ties.
#[must_use]
pub fn within_severity(a: &Finding, b: &Finding) -> Ordering {
    semantic_axes(b)
        .cmp(&semantic_axes(a))
        .then_with(|| hotspot_desc(a, b))
        .then_with(|| a.metric.cmp(&b.metric))
        .then_with(|| a.location.file.cmp(&b.location.file))
        .then_with(|| a.id.cmp(&b.id))
}

/// Full drain order. Findings without a Tier (Severity `Ok`) sort last.
#[must_use]
pub fn compare(a: &Finding, b: &Finding, drain: &PolicyDrainConfig) -> Ordering {
    let ta = drain.tier_for(a);
    let tb = drain.tier_for(b);
    let tier = match (ta, tb) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    tier.then_with(|| b.severity.cmp(&a.severity))
        .then_with(|| within_severity(a, b))
}

/// Sort `findings` into drain order in place.
pub fn sort(findings: &mut [&Finding], drain: &PolicyDrainConfig) {
    findings.sort_by(|a, b| compare(a, b, drain));
}

/// Set `drain_tier` and a family-local, 1-based `drain_rank` on every
/// non-accepted Finding that has a Tier, in exactly the order `heal
/// status` renders each family's queue (Tier, Severity, then
/// [`within_severity`]). Everything else gets `None`. Render-time only:
/// run it after `latest.json` is written and after the accepted overlay,
/// so neither field is persisted and acceptance takes effect at once.
pub fn decorate(findings: &mut [Finding], drain: &PolicyDrainConfig) {
    let mut by_family: BTreeMap<crate::feature::Family, Vec<usize>> = BTreeMap::new();
    for (i, f) in findings.iter_mut().enumerate() {
        f.drain_rank = None;
        f.drain_tier = if f.accepted { None } else { drain.tier_for(f) };
        if f.drain_tier.is_some() {
            by_family.entry(f.family()).or_default().push(i);
        }
    }
    for mut queue in by_family.into_values() {
        queue.sort_by(|&a, &b| compare(&findings[a], &findings[b], drain));
        for (rank, i) in queue.into_iter().enumerate() {
            findings[i].drain_rank = u32::try_from(rank + 1).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::DrainTier;
    use crate::core::finding::Location;
    use crate::core::severity::Severity;
    use std::path::PathBuf;

    fn f(file: &str, sev: Severity, hotspot: bool, score: Option<f64>) -> Finding {
        let mut f = Finding::new(
            "ccn",
            Location {
                file: PathBuf::from(file),
                line: Some(1),
                symbol: None,
            },
            "s".into(),
            file,
        );
        f.severity = sev;
        f.hotspot = hotspot;
        f.hotspot_score = score;
        f
    }

    #[test]
    fn tier_then_severity_then_hotspot_score() {
        let drain = PolicyDrainConfig::default();
        let must = f("a.rs", Severity::Critical, true, Some(1.0));
        let must_hot = f("b.rs", Severity::Critical, true, Some(9.0));
        let should = f("c.rs", Severity::Critical, false, None);
        let ok = f("d.rs", Severity::Ok, true, Some(99.0));
        let mut v = vec![&ok, &should, &must, &must_hot];
        sort(&mut v, &drain);
        let files: Vec<_> = v
            .iter()
            .map(|f| f.location.file.to_str().unwrap())
            .collect();
        assert_eq!(files, ["b.rs", "a.rs", "c.rs", "d.rs"]);
    }

    fn with_note(mut f: Finding, name: &str, label: &str, p: f64) -> Finding {
        f.semantic.insert(
            name.to_owned(),
            crate::core::finding::SemanticNote {
                label: label.to_owned(),
                p,
                confidence: 0.9,
                lines: Vec::new(),
                detail: None,
            },
        );
        f
    }

    #[test]
    fn decorate_ranks_each_family_in_render_order() {
        let drain = PolicyDrainConfig::default();
        let mut doc = f("docs/a.md", Severity::Critical, true, Some(1.0));
        doc.metric = "doc_drift".to_owned();
        let mut accepted = f("acc.rs", Severity::Critical, true, Some(99.0));
        accepted.accepted = true;
        let mut findings = vec![
            f("hot.rs", Severity::Critical, true, Some(50.0)),
            with_note(
                f("pay.rs", Severity::Critical, true, Some(1.0)),
                "consequence",
                "critical",
                0.9,
            ),
            f("should.rs", Severity::High, true, None),
            f("ok.rs", Severity::Ok, true, Some(99.0)),
            accepted,
            doc,
        ];
        decorate(&mut findings, &drain);
        let got: Vec<(&str, Option<DrainTier>, Option<u32>)> = findings
            .iter()
            .map(|f| {
                (
                    f.location.file.to_str().unwrap(),
                    f.drain_tier,
                    f.drain_rank,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                // The semantic `consequence` axis outranks a larger hotspot score.
                ("hot.rs", Some(DrainTier::Must), Some(2)),
                ("pay.rs", Some(DrainTier::Must), Some(1)),
                ("should.rs", Some(DrainTier::Should), Some(3)),
                ("ok.rs", None, None),
                ("acc.rs", None, None),
                // Docs is its own queue.
                ("docs/a.md", Some(DrainTier::Must), Some(1)),
            ]
        );
        let json = serde_json::to_value(&findings[1]).unwrap();
        assert_eq!(json["drain_tier"], "must");
        assert_eq!(json["drain_rank"], 1);
        assert!(serde_json::to_value(&findings[3])
            .unwrap()
            .get("drain_tier")
            .is_none());
    }

    #[test]
    fn semantic_axes_reorder_within_severity_only() {
        let drain = PolicyDrainConfig::default();
        let hot = f("hot.rs", Severity::Critical, true, Some(50.0));
        let critical_domain = with_note(
            f("pay.rs", Severity::Critical, true, Some(1.0)),
            "consequence",
            "critical",
            1.0,
        );
        let dev_tool = with_note(
            f("script.rs", Severity::Critical, true, Some(99.0)),
            "consequence",
            "dev",
            0.0,
        );
        let high = with_note(
            f("h.rs", Severity::High, true, Some(1.0)),
            "consequence",
            "critical",
            1.0,
        );
        let mut v = vec![&high, &dev_tool, &hot, &critical_domain];
        sort(&mut v, &drain);
        let files: Vec<_> = v
            .iter()
            .map(|f| f.location.file.to_str().unwrap())
            .collect();
        // Severity still dominates (h.rs is High); within Critical the
        // consequence axis beats hotspot_score, and a finding without a
        // note sits between critical and dev.
        assert_eq!(files, ["pay.rs", "hot.rs", "script.rs", "h.rs"]);
    }

    #[test]
    fn no_notes_means_plain_hotspot_order() {
        let a = f("a.rs", Severity::High, true, Some(1.0));
        let b = f("b.rs", Severity::High, true, Some(2.0));
        assert_eq!(within_severity(&a, &b), Ordering::Greater);
    }

    #[test]
    fn low_confidence_notes_are_ignored() {
        let mut n = with_note(
            f("a.rs", Severity::High, true, Some(1.0)),
            "consequence",
            "critical",
            1.0,
        );
        n.semantic.get_mut("consequence").unwrap().confidence = 0.2;
        let b = f("b.rs", Severity::High, true, Some(2.0));
        assert_eq!(within_severity(&n, &b), Ordering::Greater);
    }
}
