//! Derive user-facing findings from a `MetricsSnapshot`.
//!
//! v0.1 ships five rules, all derived from `SnapshotDelta` (which
//! `heal hook commit` precomputes). The derivation is intentionally
//! stateless — `heal hook session-start` recomputes findings on every
//! invocation and only `last_fired` is persisted, so user edits to the
//! state file at most cause an extra nudge re-fire.
//!
//! `MetricsSnapshot.delta` is the only input — see `heal-core::snapshot`
//! for the typed shape.

use crate::core::config::Config;
use crate::core::snapshot::{MetricsSnapshot, SnapshotDelta};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Severity {
    Info,
    Warn,
}

/// A single triage hint surfaced by the `SessionStart` nudge. Findings are
/// derived on demand and never persisted; the `(rule_id, subject)` pair
/// is the cool-down key so two findings for the same file/function don't
/// nudge again until the policy cool-down elapses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub rule_id: String,
    pub subject: String,
    pub severity: Severity,
    pub message: String,
}

impl Finding {
    /// Cool-down key — composed of the rule and the subject so independent
    /// files keep their own clocks.
    pub(crate) fn cooldown_key(&self) -> String {
        format!("{}:{}", self.rule_id, self.subject)
    }
}

/// Apply every rule to the supplied snapshot. Returns an empty vector when
/// the snapshot has no `delta` payload (first commit ever, or schema
/// mismatch). The resulting order matches the rule order below — callers
/// that want deterministic output can rely on that.
pub(crate) fn derive_findings(snapshot: &MetricsSnapshot, cfg: &Config) -> Vec<Finding> {
    let Some(delta_value) = snapshot.delta.as_ref() else {
        return Vec::new();
    };
    let Ok(delta) = serde_json::from_value::<SnapshotDelta>(delta_value.clone()) else {
        return Vec::new();
    };

    let mut findings = Vec::new();
    derive_hotspot(&delta, &mut findings);
    derive_complexity_top_n(&delta, &mut findings);
    derive_complexity_spike(snapshot, &delta, cfg, &mut findings);
    derive_duplication(&delta, &mut findings);
    findings
}

fn derive_hotspot(delta: &SnapshotDelta, out: &mut Vec<Finding>) {
    let Some(h) = &delta.hotspot else { return };
    for path in &h.top_files_added {
        out.push(Finding {
            rule_id: "hotspot.new_top".to_string(),
            subject: path.clone(),
            severity: Severity::Warn,
            message: format!("hotspot rank up: {path}"),
        });
    }
}

fn derive_complexity_top_n(delta: &SnapshotDelta, out: &mut Vec<Finding>) {
    let Some(c) = &delta.complexity else { return };
    for name in &c.new_top_ccn {
        out.push(Finding {
            rule_id: "complexity.new_top_ccn".to_string(),
            subject: name.clone(),
            severity: Severity::Warn,
            message: format!("CCN top-N new entry: {name}"),
        });
    }
    for name in &c.new_top_cognitive {
        out.push(Finding {
            rule_id: "complexity.new_top_cognitive".to_string(),
            subject: name.clone(),
            severity: Severity::Warn,
            message: format!("Cognitive top-N new entry: {name}"),
        });
    }
}

fn derive_complexity_spike(
    snapshot: &MetricsSnapshot,
    delta: &SnapshotDelta,
    cfg: &Config,
    out: &mut Vec<Finding>,
) {
    let Some(c) = &delta.complexity else { return };
    if c.max_ccn <= 0 {
        return;
    }
    let warn_pct = cfg.metrics.ccn.warn_delta_pct;
    if warn_pct == 0 {
        return;
    }
    let Some(curr_max) = current_max_ccn(snapshot) else {
        return;
    };
    let prev_max = i64::from(curr_max) - c.max_ccn;
    if prev_max <= 0 {
        return;
    }
    #[allow(clippy::cast_precision_loss)]
    let pct = (c.max_ccn as f64 / prev_max as f64) * 100.0;
    if pct + f64::EPSILON < f64::from(warn_pct) {
        return;
    }
    out.push(Finding {
        rule_id: "complexity.spike".to_string(),
        subject: "global".to_string(),
        severity: Severity::Warn,
        message: format!("CCN spike: max {prev_max} → {curr_max} (+{pct:.0}%)"),
    });
}

fn derive_duplication(delta: &SnapshotDelta, out: &mut Vec<Finding>) {
    let Some(d) = &delta.duplication else { return };
    if d.duplicate_blocks <= 0 {
        return;
    }
    out.push(Finding {
        rule_id: "duplication.growth".to_string(),
        subject: "global".to_string(),
        severity: Severity::Info,
        message: format!(
            "duplication grew: +{} blocks (+{} tokens)",
            d.duplicate_blocks, d.duplicate_tokens
        ),
    });
}

/// Pull `complexity.totals.max_ccn` out of the opaque `complexity` payload
/// without forcing the whole observer report into the type system.
fn current_max_ccn(snapshot: &MetricsSnapshot) -> Option<u32> {
    let v = snapshot.complexity.as_ref()?;
    v.get("totals")?
        .get("max_ccn")?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{CcnConfig, Config, MetricsConfig};
    use crate::core::snapshot::{
        ComplexityDelta, DuplicationDelta, HotspotDelta, MetricsSnapshot, SnapshotDelta,
    };
    use serde_json::json;

    fn snapshot_with_delta(delta: &SnapshotDelta) -> MetricsSnapshot {
        MetricsSnapshot {
            delta: Some(serde_json::to_value(delta).unwrap()),
            ..MetricsSnapshot::default()
        }
    }

    fn cfg_with_warn_pct(pct: u32) -> Config {
        Config {
            metrics: MetricsConfig {
                ccn: CcnConfig {
                    enabled: true,
                    warn_delta_pct: pct,
                    top_n: None,
                },
                ..MetricsConfig::default()
            },
            ..Config::default()
        }
    }

    #[test]
    fn no_delta_returns_no_findings() {
        let snap = MetricsSnapshot::default();
        assert!(derive_findings(&snap, &Config::default()).is_empty());
    }

    #[test]
    fn hotspot_new_top_emits_per_file() {
        let snap = snapshot_with_delta(&SnapshotDelta {
            hotspot: Some(HotspotDelta {
                max_score: 0.0,
                top_files_added: vec!["src/a.rs".into(), "src/b.rs".into()],
                top_files_dropped: vec![],
            }),
            ..SnapshotDelta::default()
        });
        let findings = derive_findings(&snap, &Config::default());
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].rule_id, "hotspot.new_top");
        assert_eq!(findings[0].subject, "src/a.rs");
        assert_eq!(findings[1].subject, "src/b.rs");
    }

    #[test]
    fn complexity_top_n_emits_ccn_and_cognitive_separately() {
        let snap = snapshot_with_delta(&SnapshotDelta {
            complexity: Some(ComplexityDelta {
                new_top_ccn: vec!["fn_one".into()],
                new_top_cognitive: vec!["fn_two".into()],
                ..ComplexityDelta::default()
            }),
            ..SnapshotDelta::default()
        });
        let findings = derive_findings(&snap, &Config::default());
        let rules: Vec<&str> = findings.iter().map(|f| f.rule_id.as_str()).collect();
        assert!(rules.contains(&"complexity.new_top_ccn"));
        assert!(rules.contains(&"complexity.new_top_cognitive"));
    }

    #[test]
    fn complexity_spike_fires_above_threshold() {
        let mut snap = snapshot_with_delta(&SnapshotDelta {
            complexity: Some(ComplexityDelta {
                // prev=10 → curr=15 → +50%
                max_ccn: 5,
                ..ComplexityDelta::default()
            }),
            ..SnapshotDelta::default()
        });
        snap.complexity = Some(json!({ "totals": { "max_ccn": 15 } }));
        let findings = derive_findings(&snap, &cfg_with_warn_pct(30));
        assert!(findings
            .iter()
            .any(|f| f.rule_id == "complexity.spike" && f.subject == "global"));
    }

    #[test]
    fn complexity_spike_silent_below_threshold() {
        let mut snap = snapshot_with_delta(&SnapshotDelta {
            complexity: Some(ComplexityDelta {
                // prev=20 → curr=22 → +10%, below 30%
                max_ccn: 2,
                ..ComplexityDelta::default()
            }),
            ..SnapshotDelta::default()
        });
        snap.complexity = Some(json!({ "totals": { "max_ccn": 22 } }));
        let findings = derive_findings(&snap, &cfg_with_warn_pct(30));
        assert!(findings.iter().all(|f| f.rule_id != "complexity.spike"));
    }

    #[test]
    fn duplication_growth_only_when_blocks_increase() {
        let snap = snapshot_with_delta(&SnapshotDelta {
            duplication: Some(DuplicationDelta {
                duplicate_blocks: 3,
                duplicate_tokens: 120,
                files_affected: 2,
            }),
            ..SnapshotDelta::default()
        });
        let findings = derive_findings(&snap, &Config::default());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "duplication.growth");

        let snap_neutral = snapshot_with_delta(&SnapshotDelta {
            duplication: Some(DuplicationDelta::default()),
            ..SnapshotDelta::default()
        });
        let findings = derive_findings(&snap_neutral, &Config::default());
        assert!(findings.is_empty());
    }

    #[test]
    fn cooldown_key_includes_rule_and_subject() {
        let f = Finding {
            rule_id: "hotspot.new_top".into(),
            subject: "src/a.rs".into(),
            severity: Severity::Warn,
            message: String::new(),
        };
        assert_eq!(f.cooldown_key(), "hotspot.new_top:src/a.rs");
    }
}
