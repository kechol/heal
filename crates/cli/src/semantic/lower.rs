//! The offline half of `[features.semantic]`: turn cached verdicts into
//! Findings and decorations during `heal status` / `heal diff`.
//!
//! Runs after `FeatureRegistry::lower_all` (the ordinary families) so
//! tasks can pick subjects from classified, hotspot-decorated Findings.
//! Never touches the network: an item without a cached verdict is simply
//! skipped, which is exactly what a teammate without an API key sees for
//! code nobody has asked about yet.

use std::collections::BTreeMap;
use std::path::Path;

use crate::core::config::Config;
use crate::core::finding::Finding;
use crate::feature::Family;
use crate::observers::ObserverReports;
use crate::semantic::store::VerdictStore;
use crate::semantic::task::{registry, Answered, Task, TaskContext};

/// Apply every enabled task's lowering to `findings`. Tasks that fail to
/// plan (unreadable file, broken cache line) are reported on stderr and
/// skipped; a semantic problem must never break `heal status`.
pub(crate) fn apply(
    scan_root: &Path,
    cfg: &Config,
    reports: &ObserverReports,
    findings: Vec<Finding>,
) -> Vec<Finding> {
    if !cfg.features.semantic.enabled {
        return findings;
    }
    let tasks = registry();
    let enabled: Vec<&dyn Task> = tasks
        .iter()
        .map(AsRef::as_ref)
        .filter(|t| cfg.features.semantic.task_enabled(t.id()) && !t.on_demand())
        .collect();
    if enabled.is_empty() {
        return findings;
    }
    let mut store = VerdictStore::new(crate::core::HealPaths::new(scan_root).semantic_verdicts());
    let (new_findings, notes) = {
        let Ok(ctx) = TaskContext::new(scan_root, cfg) else {
            return findings;
        };
        let ctx = ctx.with_base(reports, &findings);
        let mut new_findings = Vec::new();
        let mut notes = Vec::new();
        for task in enabled {
            let snapshot = match store.snapshot(task.depends_on()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("heal: semantic task `{}` skipped: {e}", task.id());
                    continue;
                }
            };
            let tctx = ctx.with_prior(&snapshot);
            match lower_task(task, &tctx, &mut store) {
                Ok(lowered) => {
                    new_findings.extend(lowered.findings);
                    notes.extend(lowered.notes);
                }
                Err(e) => eprintln!("heal: semantic task `{}` skipped: {e}", task.id()),
            }
        }
        (new_findings, notes)
    };
    merge(cfg, findings, new_findings, notes)
}

fn lower_task(
    task: &dyn Task,
    ctx: &TaskContext<'_>,
    store: &mut VerdictStore,
) -> anyhow::Result<crate::semantic::task::Lowered> {
    let groups = task.plan(ctx)?;
    let mut answers = BTreeMap::new();
    for group in &groups {
        for item in &group.items {
            if let Some(v) = store
                .get(task.id(), &item.key)
                .map_err(anyhow::Error::msg)?
            {
                answers.insert(item.key.clone(), v.answer);
            }
        }
    }
    let answered: Vec<Answered<'_>> = groups
        .iter()
        .flat_map(|g| g.items.iter())
        .map(|item| Answered {
            item,
            answer: answers.get(&item.key),
        })
        .collect();
    Ok(task.lower(ctx, &answered))
}

/// Attach notes by id, then append new Findings. New Findings inherit the
/// per-family hotspot decoration and the test-file tag from any existing
/// Finding on the same file, so they sort and filter like their
/// neighbours. Output order is deterministic: existing order, then new
/// Findings sorted by id.
fn merge(
    cfg: &Config,
    mut findings: Vec<Finding>,
    mut new_findings: Vec<Finding>,
    notes: Vec<(String, String, crate::core::finding::SemanticNote)>,
) -> Vec<Finding> {
    let index: BTreeMap<String, usize> = findings
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();
    for (id, name, note) in notes {
        if let Some(&i) = index.get(&id) {
            findings[i].semantic.insert(name, note);
        }
    }
    for f in &mut new_findings {
        let family = Family::for_metric(&f.metric);
        for base in findings
            .iter()
            .filter(|b| b.location.file == f.location.file)
        {
            if Family::for_metric(&base.metric) == family && base.hotspot {
                f.hotspot = true;
                f.hotspot_score = match (f.hotspot_score, base.hotspot_score) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
            }
            f.is_test_file |= base.is_test_file;
        }
        if !cfg.project.workspaces.is_empty() {
            f.workspace =
                crate::core::config::assign_workspace(&f.location.file, &cfg.project.workspaces)
                    .map(str::to_owned);
        }
    }
    new_findings.sort_by(|a, b| a.id.cmp(&b.id));
    new_findings.dedup_by(|a, b| a.id == b.id);
    findings.extend(new_findings);
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::finding::{Location, SemanticNote};
    use crate::core::severity::Severity;
    use std::path::PathBuf;

    fn finding(metric: &str, file: &str, hotspot: bool) -> Finding {
        let mut f = Finding::new(
            metric,
            Location {
                file: PathBuf::from(file),
                line: Some(1),
                symbol: None,
            },
            "s".into(),
            metric,
        );
        f.hotspot = hotspot;
        f.hotspot_score = hotspot.then_some(10.0);
        f
    }

    #[test]
    fn disabled_family_is_a_no_op() {
        let cfg = Config::default();
        let base = vec![finding("ccn", "a.rs", true)];
        let out = apply(
            Path::new("/nonexistent"),
            &cfg,
            &ObserverReports::default(),
            base.clone(),
        );
        assert_eq!(out, base);
    }

    #[test]
    fn merge_attaches_notes_and_inherits_hotspot() {
        let cfg = Config::default();
        let base = vec![finding("ccn", "a.rs", true), finding("ccn", "b.rs", false)];
        let id = base[0].id.clone();
        let mut new = finding("concept_mix", "a.rs", false);
        new.severity = Severity::High;
        let note = SemanticNote {
            label: "local".into(),
            p: 0.9,
            confidence: 0.8,
            lines: vec![],
            detail: None,
        };
        let out = merge(
            &cfg,
            base,
            vec![new],
            vec![
                (id.clone(), "effort".into(), note.clone()),
                ("missing".into(), "x".into(), note.clone()),
            ],
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].semantic.get("effort"), Some(&note));
        assert!(
            out[2].hotspot,
            "new finding inherits the code-family hotspot of its file"
        );
        assert_eq!(out[2].hotspot_score, Some(10.0));
    }
}
