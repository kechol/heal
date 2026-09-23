//! `heal semantic ask`: plan → skip cached → pack → price → send → store.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::semantic::api::Answer;
use crate::semantic::client::{JevClient, JevErrorKind, Spend};
use crate::semantic::cost::usd_for;
use crate::semantic::plan::{pack, Batch, PackError};
use crate::semantic::store::{Verdict, VerdictStore};
use crate::semantic::task::{Task, TaskContext};

#[derive(Debug, Clone, Copy, Default)]
pub struct AskOptions {
    /// Plan and price only; send nothing, write nothing.
    pub dry_run: bool,
    /// Re-ask subjects that already have a cached verdict.
    pub refresh: bool,
    /// Drop cached verdicts no current subject refers to.
    pub prune: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskReport {
    pub task: String,
    pub subjects: usize,
    pub cached: usize,
    pub to_ask: usize,
    pub requests: usize,
    pub est_input_tokens: usize,
    pub est_usd: f64,
    pub answered: usize,
    pub failed: usize,
    /// Groups whose shared state alone exceeds the state ceiling.
    pub oversized_groups: usize,
    pub pruned: usize,
    /// Why nothing was planned, when the user can fix it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AskReport {
    pub model: String,
    pub dry_run: bool,
    pub tasks: Vec<TaskReport>,
    pub requests_sent: u64,
    pub input_tokens: u64,
    pub usd: f64,
    /// Dispatch stopped before a batch that would cross `max_usd`.
    pub stopped_by_budget: bool,
    /// First fatal error (bad key, no credit). Remaining batches were skipped.
    pub fatal: Option<String>,
    pub errors: Vec<String>,
}

struct Planned {
    report: TaskReport,
    batches: Vec<Batch>,
    keep: BTreeSet<String>,
}

fn plan_task(
    task: &dyn Task,
    ctx: &TaskContext<'_>,
    store: &mut VerdictStore,
    opts: AskOptions,
) -> anyhow::Result<Planned> {
    let mut report = TaskReport {
        task: task.id().to_owned(),
        ..TaskReport::default()
    };
    let mut keep = BTreeSet::new();
    let mut batches = Vec::new();
    for group in task.plan(ctx)? {
        let mut pending = Vec::new();
        for item in group.items {
            if !keep.insert(item.key.clone()) {
                continue;
            }
            report.subjects += 1;
            if !opts.refresh
                && store
                    .contains(task.id(), &item.key)
                    .map_err(anyhow::Error::msg)?
            {
                report.cached += 1;
                continue;
            }
            pending.push(item);
        }
        if pending.is_empty() {
            continue;
        }
        let n = pending.len();
        match pack(group.state, pending) {
            Ok(mut b) => {
                report.to_ask += n;
                batches.append(&mut b);
            }
            Err(PackError::StateTooLarge { .. }) => {
                report.oversized_groups += 1;
            }
        }
    }
    if report.subjects == 0 {
        report.hint = task.setup_hint(ctx);
    }
    report.requests = batches.len();
    report.est_input_tokens = batches.iter().map(|b| b.est_tokens).sum();
    report.est_usd = usd_for(report.est_input_tokens as u64);
    Ok(Planned {
        report,
        batches,
        keep,
    })
}

/// Run `tasks`. `client` is `None` for a dry run.
pub fn run(
    tasks: &[&dyn Task],
    ctx: &TaskContext<'_>,
    store: &mut VerdictStore,
    client: Option<&JevClient>,
    opts: AskOptions,
) -> anyhow::Result<AskReport> {
    let semantic = ctx.semantic();
    let mut report = AskReport {
        model: semantic.model.clone(),
        dry_run: opts.dry_run || client.is_none(),
        ..AskReport::default()
    };
    let mut planned = Vec::new();
    for task in tasks {
        planned.push(plan_task(*task, ctx, store, opts)?);
    }
    if opts.prune && !report.dry_run {
        for p in &mut planned {
            p.report.pruned = store
                .prune(&p.report.task, &p.keep)
                .map_err(anyhow::Error::msg)?;
        }
    }
    let Some(client) = client.filter(|_| !report.dry_run) else {
        report.tasks = planned.into_iter().map(|p| p.report).collect();
        return Ok(report);
    };

    let queue: Vec<(usize, &Batch)> = planned
        .iter()
        .enumerate()
        .flat_map(|(ti, p)| p.batches.iter().map(move |b| (ti, b)))
        .collect();
    let out = dispatch(
        &queue,
        planned.len(),
        client,
        semantic.max_usd,
        semantic.concurrency,
    );

    for (ti, key, answer) in out.answers {
        planned[ti].report.answered += 1;
        store
            .insert(
                &planned[ti].report.task,
                Verdict {
                    key,
                    model: semantic.model.clone(),
                    answer,
                },
            )
            .map_err(anyhow::Error::msg)?;
    }
    for (p, f) in planned.iter_mut().zip(out.failed) {
        p.report.failed = f;
    }
    let spend: Spend = client.spend();
    report.requests_sent = spend.calls;
    report.input_tokens = spend.input_tokens;
    report.usd = spend.usd();
    report.stopped_by_budget = out.over_budget;
    report.fatal = out.fatal;
    report.errors = out.errors;
    report.tasks = planned.into_iter().map(|p| p.report).collect();
    Ok(report)
}

struct Dispatched {
    answers: Vec<(usize, String, Answer)>,
    failed: Vec<usize>,
    over_budget: bool,
    fatal: Option<String>,
    errors: Vec<String>,
}

/// Send `queue` with up to `concurrency` requests in flight. Each batch
/// reserves its estimated price before it is sent; the batch that would
/// cross `budget` and everything after it stay unsent. An auth failure
/// (bad key, no credit) stops every worker, since every batch would fail
/// the same way.
fn dispatch(
    queue: &[(usize, &Batch)],
    task_count: usize,
    client: &JevClient,
    budget: f64,
    concurrency: usize,
) -> Dispatched {
    let reserved = Mutex::new(0.0_f64);
    let stop = AtomicBool::new(false);
    let over_budget = AtomicBool::new(false);
    let fatal: Mutex<Option<String>> = Mutex::new(None);
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let answers: Mutex<Vec<(usize, String, Answer)>> = Mutex::new(Vec::new());
    let failed: Vec<AtomicUsize> = (0..task_count).map(|_| AtomicUsize::new(0)).collect();
    let next = AtomicUsize::new(0);
    let workers = concurrency.min(queue.len()).max(1);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(&(ti, batch)) = queue.get(i) else {
                    return;
                };
                let est = usd_for(batch.est_tokens as u64);
                {
                    let mut r = reserved.lock().expect("reserve lock");
                    if *r + est > budget {
                        over_budget.store(true, Ordering::SeqCst);
                        stop.store(true, Ordering::SeqCst);
                        return;
                    }
                    *r += est;
                }
                match client.ask_splitting(&batch.state, &batch.questions) {
                    Ok(res) => {
                        let mut out = answers.lock().expect("answers lock");
                        for key in batch.questions.keys() {
                            if let Some(a) = res.answers.get(key) {
                                out.push((ti, key.clone(), a.clone()));
                            } else {
                                failed[ti].fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    }
                    Err(e) => {
                        failed[ti].fetch_add(batch.questions.len(), Ordering::SeqCst);
                        if e.kind == JevErrorKind::Auth {
                            stop.store(true, Ordering::SeqCst);
                            fatal
                                .lock()
                                .expect("fatal lock")
                                .get_or_insert(e.to_string());
                        } else {
                            errors.lock().expect("errors lock").push(e.to_string());
                        }
                    }
                }
            });
        }
    });

    let mut answers = answers.into_inner().expect("answers");
    // Worker interleaving must not leak into the store's insert order.
    answers.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    Dispatched {
        answers,
        failed: failed.iter().map(|f| f.load(Ordering::SeqCst)).collect(),
        over_budget: over_budget.load(Ordering::SeqCst),
        fatal: fatal.into_inner().expect("fatal"),
        errors: errors.into_inner().expect("errors"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::semantic::api::Question;
    use crate::semantic::client::testing::{answer_all_noul, FakeTransport};
    use crate::semantic::client::HttpResponse;
    use crate::semantic::task::{Group, Item};
    use serde_json::json;
    use std::sync::Arc;

    struct Fixed(usize);

    impl Task for Fixed {
        fn id(&self) -> &'static str {
            "fixed"
        }
        fn summary(&self) -> &'static str {
            "test task"
        }
        fn criteria_text(&self) -> String {
            "is it?".to_owned()
        }
        fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
            let items = (0..self.0)
                .map(|i| Item {
                    key: ctx.key(self, &format!("subject{i}"), "state"),
                    question: Question::Noul {
                        instructions: json!(format!("is {i} odd?")),
                        criteria: None,
                    },
                    meta: serde_json::Value::Null,
                })
                .collect();
            Ok(vec![Group {
                state: json!("state"),
                items,
            }])
        }
    }

    fn client(
        handler: impl Fn(&str, &[u8]) -> Result<HttpResponse, String> + Send + Sync + 'static,
    ) -> JevClient {
        JevClient::new(
            Arc::new(FakeTransport::new(handler)),
            "k".into(),
            "jev-1.13.0".into(),
        )
        .with_base_url("http://test")
        .with_sleeper(|_| {})
    }

    #[test]
    fn asks_once_then_serves_from_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::default();
        let ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        let mut store = VerdictStore::new(dir.path().join("v"));
        let c = client(|_, body| Ok(answer_all_noul(body, 0.8)));
        let task = Fixed(3);

        let r = run(&[&task], &ctx, &mut store, Some(&c), AskOptions::default()).unwrap();
        assert_eq!(r.tasks[0].answered, 3);
        store.save().unwrap();

        let r = run(&[&task], &ctx, &mut store, Some(&c), AskOptions::default()).unwrap();
        assert_eq!(r.tasks[0].cached, 3);
        assert_eq!(r.tasks[0].to_ask, 0);
        assert_eq!(c.spend().calls, 1, "second run must not send");
    }

    #[test]
    fn dry_run_prices_without_sending_or_writing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::default();
        let ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        let mut store = VerdictStore::new(dir.path().join("v"));
        let r = run(
            &[&Fixed(5)],
            &ctx,
            &mut store,
            None,
            AskOptions {
                dry_run: true,
                ..AskOptions::default()
            },
        )
        .unwrap();
        assert!(r.dry_run);
        assert_eq!(r.tasks[0].to_ask, 5);
        assert!(r.tasks[0].est_usd > 0.0);
        assert!(store.save().unwrap().is_empty());
    }

    #[test]
    fn budget_guard_stops_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.features.semantic.max_usd = 1e-12;
        let ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        let mut store = VerdictStore::new(dir.path().join("v"));
        let c = client(|_, body| Ok(answer_all_noul(body, 0.8)));
        let r = run(
            &[&Fixed(2)],
            &ctx,
            &mut store,
            Some(&c),
            AskOptions::default(),
        )
        .unwrap();
        assert!(r.stopped_by_budget);
        assert_eq!(c.spend().calls, 0);
    }

    #[test]
    fn auth_failure_is_fatal_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::default();
        let ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        let mut store = VerdictStore::new(dir.path().join("v"));
        let c = client(|_, _| {
            Ok(HttpResponse {
                status: 402,
                body: "no credit".into(),
                retry_after: None,
            })
        });
        let r = run(
            &[&Fixed(2)],
            &ctx,
            &mut store,
            Some(&c),
            AskOptions::default(),
        )
        .unwrap();
        assert!(r.fatal.is_some());
        assert_eq!(r.tasks[0].failed, 2);
    }

    #[test]
    fn prune_keeps_only_current_subjects() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::default();
        let ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        let mut store = VerdictStore::new(dir.path().join("v"));
        let c = client(|_, body| Ok(answer_all_noul(body, 0.8)));
        run(
            &[&Fixed(4)],
            &ctx,
            &mut store,
            Some(&c),
            AskOptions::default(),
        )
        .unwrap();
        let r = run(
            &[&Fixed(2)],
            &ctx,
            &mut store,
            Some(&c),
            AskOptions {
                prune: true,
                ..AskOptions::default()
            },
        )
        .unwrap();
        assert_eq!(r.tasks[0].pruned, 2);
        assert_eq!(store.len("fixed").unwrap(), 2);
    }
}
