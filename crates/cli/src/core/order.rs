//! Drain-queue order: the single definition shared by `heal status`, the
//! patch skills (through `heal status --json`), and the Q9 backtest.
//!
//! Tier, then Severity, then the family-local `hotspot_score`, then
//! deterministic metric / path / id ties. When `[features.semantic]`
//! verdicts are present, their ordering axes slot in between Severity
//! and `hotspot_score` (see [`within_severity`]); they never change a
//! Finding's Tier or Severity, so `design-philosophy.md` §1.3 (Severity
//! and Hotspot stay orthogonal) holds.

use std::cmp::Ordering;

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

/// Order of two Findings that already share Tier and Severity.
#[must_use]
pub fn within_severity(a: &Finding, b: &Finding) -> Ordering {
    hotspot_desc(a, b)
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
