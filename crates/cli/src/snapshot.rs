//! Package observer reports into a `MetricsSnapshot` for the commit hook
//! to persist. Pure glue over `crate::observers`.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use heal_core::config::{load_from_project, MetricsConfig};
use heal_core::history::{
    ChangeCouplingDelta, ChurnDelta, ComplexityDelta, DuplicationDelta, HistoryReader,
    HotspotDelta, MetricsSnapshot, SnapshotDelta, METRICS_SNAPSHOT_VERSION,
};
use heal_core::HealPaths;
use heal_observer::change_coupling::ChangeCouplingReport;
use heal_observer::churn::ChurnReport;
use heal_observer::complexity::{ComplexityMetric, ComplexityReport};
use heal_observer::duplication::DuplicationReport;
use heal_observer::hotspot::HotspotReport;

use crate::observers::{run_all, ObserverReports};

/// `capture` plus serialization to the opaque JSON used as the `Snapshot`
/// `data` payload. Both `heal init` and `heal hook commit` write the same
/// shape, so the conversion lives here once.
pub(crate) fn capture_value(project: &Path) -> Result<serde_json::Value> {
    let snap = capture(project)?;
    Ok(serde_json::to_value(&snap).expect("MetricsSnapshot serialization is infallible"))
}

/// Run every enabled observer and package the results into a snapshot.
///
/// Returns `Ok(MetricsSnapshot::default())` when the project hasn't been
/// initialized yet (no `.heal/config.toml`). This keeps the commit hook
/// lossless even before `heal init` lands.
pub(crate) fn capture(project: &Path) -> Result<MetricsSnapshot> {
    let cfg = match load_from_project(project) {
        Ok(c) => c,
        Err(heal_core::Error::ConfigMissing(_)) => return Ok(MetricsSnapshot::default()),
        Err(e) => return Err(e.into()),
    };
    let paths = HealPaths::new(project);
    let reports = run_all(project, &cfg);
    let mut snap = pack(project, &reports);

    // Best-effort delta against the previous snapshot. Failures here are
    // non-fatal — the hook still records the new snapshot, just without a
    // diff payload.
    let reader = HistoryReader::new(paths.history_dir());
    if let Ok(Some((prev_snap, prev_metrics))) = reader.latest_metrics_snapshot() {
        let delta = compute_delta(prev_snap.timestamp, &prev_metrics, &reports, &cfg.metrics);
        snap.delta =
            Some(serde_json::to_value(&delta).expect("SnapshotDelta serialization is infallible"));
    }
    Ok(snap)
}

fn pack(project: &Path, reports: &ObserverReports) -> MetricsSnapshot {
    MetricsSnapshot {
        version: METRICS_SNAPSHOT_VERSION,
        git_sha: heal_observer::git::head_sha(project),
        loc: Some(to_value(&reports.loc)),
        complexity: Some(to_value(&reports.complexity)),
        churn: reports.churn.as_ref().map(to_value),
        change_coupling: reports.change_coupling.as_ref().map(to_value),
        duplication: reports.duplication.as_ref().map(to_value),
        hotspot: reports.hotspot.as_ref().map(to_value),
        delta: None,
    }
}

fn to_value<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("observer report serialization is infallible")
}

fn decode<T: serde::de::DeserializeOwned>(v: Option<&serde_json::Value>) -> Option<T> {
    v.and_then(|val| serde_json::from_value(val.clone()).ok())
}

/// Compose every per-metric delta. `metrics` supplies the per-metric `top_n`
/// values that drive "entered / dropped from top-N" comparisons.
fn compute_delta(
    prev_ts: chrono::DateTime<chrono::Utc>,
    prev: &MetricsSnapshot,
    curr: &ObserverReports,
    metrics: &MetricsConfig,
) -> SnapshotDelta {
    SnapshotDelta {
        from_sha: prev.git_sha.clone(),
        from_timestamp: Some(prev_ts),
        complexity: decode::<ComplexityReport>(prev.complexity.as_ref())
            .map(|p| complexity_delta(&p, &curr.complexity, metrics.top_n_complexity())),
        churn: pair_curr(prev.churn.as_ref(), curr.churn.as_ref()).map(|(p, c)| churn_delta(&p, c)),
        hotspot: pair_curr(prev.hotspot.as_ref(), curr.hotspot.as_ref())
            .map(|(p, c)| hotspot_delta(&p, c, metrics.top_n_hotspot())),
        duplication: pair_curr(prev.duplication.as_ref(), curr.duplication.as_ref())
            .map(|(p, c)| duplication_delta(&p, c)),
        change_coupling: pair_curr(prev.change_coupling.as_ref(), curr.change_coupling.as_ref())
            .map(|(p, c)| change_coupling_delta(&p, c)),
    }
}

/// Pair a previous opaque `Value` with the current typed report, decoding
/// only the previous side. Returns `None` if either side is absent or the
/// previous payload's shape no longer matches.
fn pair_curr<'a, T: serde::de::DeserializeOwned>(
    prev: Option<&serde_json::Value>,
    curr: Option<&'a T>,
) -> Option<(T, &'a T)> {
    Some((decode::<T>(prev)?, curr?))
}

fn delta_i64(curr: usize, prev: usize) -> i64 {
    i64::try_from(curr).unwrap_or(i64::MAX) - i64::try_from(prev).unwrap_or(i64::MAX)
}

fn top_names(report: &ComplexityReport, n: usize, metric: ComplexityMetric) -> Vec<String> {
    report
        .worst_n(n, metric)
        .into_iter()
        .map(|f| f.name)
        .collect()
}

fn complexity_delta(
    prev: &ComplexityReport,
    curr: &ComplexityReport,
    top_n: usize,
) -> ComplexityDelta {
    let prev_ccn: BTreeSet<String> = top_names(prev, top_n, ComplexityMetric::Ccn)
        .into_iter()
        .collect();
    let curr_ccn = top_names(curr, top_n, ComplexityMetric::Ccn);
    let new_top_ccn: Vec<String> = curr_ccn
        .into_iter()
        .filter(|n| !prev_ccn.contains(n))
        .collect();

    let prev_cog: BTreeSet<String> = top_names(prev, top_n, ComplexityMetric::Cognitive)
        .into_iter()
        .collect();
    let curr_cog = top_names(curr, top_n, ComplexityMetric::Cognitive);
    let new_top_cognitive: Vec<String> = curr_cog
        .into_iter()
        .filter(|n| !prev_cog.contains(n))
        .collect();

    ComplexityDelta {
        max_ccn: i64::from(curr.totals.max_ccn) - i64::from(prev.totals.max_ccn),
        max_cognitive: i64::from(curr.totals.max_cognitive) - i64::from(prev.totals.max_cognitive),
        functions: delta_i64(curr.totals.functions, prev.totals.functions),
        files: delta_i64(curr.totals.files, prev.totals.files),
        new_top_ccn,
        new_top_cognitive,
    }
}

fn churn_delta(prev: &ChurnReport, curr: &ChurnReport) -> ChurnDelta {
    let top = |r: &ChurnReport| {
        r.worst_n(1)
            .into_iter()
            .next()
            .map(|f| f.path.display().to_string())
    };
    let prev_top = top(prev);
    let curr_top = top(curr);
    ChurnDelta {
        commits_in_window: i64::from(curr.totals.commits) - i64::from(prev.totals.commits),
        top_file_changed: prev_top != curr_top,
        previous_top_file: prev_top,
        current_top_file: curr_top,
    }
}

fn hotspot_delta(prev: &HotspotReport, curr: &HotspotReport, top_n: usize) -> HotspotDelta {
    let names = |r: &HotspotReport| -> BTreeSet<String> {
        r.worst_n(top_n)
            .into_iter()
            .map(|e| e.path.display().to_string())
            .collect()
    };
    let prev_top = names(prev);
    let curr_top = names(curr);
    HotspotDelta {
        max_score: curr.totals.max_score - prev.totals.max_score,
        top_files_added: curr_top.difference(&prev_top).cloned().collect(),
        top_files_dropped: prev_top.difference(&curr_top).cloned().collect(),
    }
}

fn duplication_delta(prev: &DuplicationReport, curr: &DuplicationReport) -> DuplicationDelta {
    DuplicationDelta {
        duplicate_blocks: delta_i64(curr.totals.duplicate_blocks, prev.totals.duplicate_blocks),
        duplicate_tokens: delta_i64(curr.totals.duplicate_tokens, prev.totals.duplicate_tokens),
        files_affected: delta_i64(curr.totals.files_affected, prev.totals.files_affected),
    }
}

fn change_coupling_delta(
    prev: &ChangeCouplingReport,
    curr: &ChangeCouplingReport,
) -> ChangeCouplingDelta {
    let prev_max = prev.pairs.iter().map(|p| p.count).max().unwrap_or(0);
    let curr_max = curr.pairs.iter().map(|p| p.count).max().unwrap_or(0);
    ChangeCouplingDelta {
        pairs: delta_i64(curr.totals.pairs, prev.totals.pairs),
        files: delta_i64(curr.totals.files, prev.totals.files),
        max_pair_count: i64::from(curr_max) - i64::from(prev_max),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heal_observer::churn::{ChurnTotals, FileChurn};
    use heal_observer::complexity::{ComplexityTotals, FileComplexity, FunctionMetric};
    use heal_observer::hotspot::{HotspotEntry, HotspotTotals};

    fn complexity_with(max_ccn: u32, max_cog: u32, functions: usize) -> ComplexityReport {
        ComplexityReport {
            files: vec![FileComplexity {
                path: "src/lib.rs".into(),
                language: "rust".into(),
                functions: (0..functions)
                    .map(|i| FunctionMetric {
                        name: format!("f{i}"),
                        start_line: 1,
                        end_line: 1,
                        ccn: max_ccn,
                        cognitive: max_cog,
                    })
                    .collect(),
            }],
            totals: ComplexityTotals {
                files: 1,
                functions,
                max_ccn,
                max_cognitive: max_cog,
            },
        }
    }

    #[test]
    fn complexity_delta_captures_max_movement() {
        let prev = complexity_with(10, 5, 3);
        let curr = complexity_with(13, 4, 5);
        let d = complexity_delta(&prev, &curr, 5);
        assert_eq!(d.max_ccn, 3);
        assert_eq!(d.max_cognitive, -1);
        assert_eq!(d.functions, 2);
    }

    #[test]
    fn churn_delta_flags_top_file_change() {
        let prev = ChurnReport {
            files: vec![FileChurn {
                path: "a.rs".into(),
                commits: 3,
                lines_added: 0,
                lines_deleted: 0,
            }],
            totals: ChurnTotals {
                files: 1,
                commits: 3,
                lines_added: 0,
                lines_deleted: 0,
            },
            since_days: 30,
        };
        let curr = ChurnReport {
            files: vec![FileChurn {
                path: "b.rs".into(),
                commits: 5,
                lines_added: 0,
                lines_deleted: 0,
            }],
            totals: ChurnTotals {
                files: 1,
                commits: 5,
                lines_added: 0,
                lines_deleted: 0,
            },
            since_days: 30,
        };
        let d = churn_delta(&prev, &curr);
        assert_eq!(d.commits_in_window, 2);
        assert!(d.top_file_changed);
        assert_eq!(d.previous_top_file.as_deref(), Some("a.rs"));
        assert_eq!(d.current_top_file.as_deref(), Some("b.rs"));
    }

    #[test]
    fn hotspot_delta_tracks_top_n_membership() {
        let mk = |entries: &[(&str, f64)]| HotspotReport {
            entries: entries
                .iter()
                .map(|(p, s)| HotspotEntry {
                    path: (*p).into(),
                    ccn_sum: 1,
                    churn_commits: 1,
                    score: *s,
                })
                .collect(),
            totals: HotspotTotals {
                files: entries.len(),
                max_score: entries.first().map_or(0.0, |(_, s)| *s),
            },
        };
        let prev = mk(&[("a.rs", 10.0), ("b.rs", 8.0)]);
        let curr = mk(&[("b.rs", 12.0), ("c.rs", 9.0)]);
        let d = hotspot_delta(&prev, &curr, 2);
        assert!((d.max_score - 2.0).abs() < f64::EPSILON);
        assert_eq!(d.top_files_added, vec!["c.rs".to_string()]);
        assert_eq!(d.top_files_dropped, vec!["a.rs".to_string()]);
    }
}
