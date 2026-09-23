//! Dev-only backtest of HEAL's drain order.
//!
//! Not a user-facing command: it exists so maintainers can check, with
//! numbers, whether a change to the ordering (for example a new
//! `[features.semantic]` axis) ranks the files that later needed bug fixes
//! higher than the current order does.
//!
//! For each point in time T:
//!
//! 1. Check out T in a temporary `git worktree` and observe it with a
//!    calibration computed from T itself (no future thresholds leak in).
//! 2. Rank source files four ways: HEAL's drain order
//!    (`core::order::compare`), churn only, LOC only, and a deterministic
//!    pseudo-random order.
//! 3. Label each file with the number of bug-fix commits that touched it
//!    in (T, T + horizon]. A commit is a fix when its Conventional-Commit
//!    type is `fix` or its message says fix / bug / hotfix / regression —
//!    never a Jev verdict, so a semantic axis is not graded by itself.
//!    Commits HEAL's own patch skill made (`(heal)` scope) are ignored:
//!    the ranking being tested produced them.
//! 4. Report precision@k and the share of fix touches found after reading
//!    20% of the LOC in rank order (effort-aware evaluation, after Mende &
//!    Koschke, "Effort-Aware Defect Prediction Models", CSMR 2010).
//!
//! Usage:
//!
//! ```sh
//! cargo run --example backtest -- --project . --points 4 --step-days 90 --horizon-days 180
//! cargo run --example backtest -- --at v0.5.0 --at v0.4.0 --with-verdicts
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use git2::{Oid, Repository, Sort};

use heal_cli::core::config::Config;
use heal_cli::core::finding::Finding;
use heal_cli::core::hash::fnv1a_64;
use heal_cli::core::severity::Severity;

#[derive(Debug, Parser)]
struct Args {
    /// Repository to evaluate.
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Explicit points in time (any git revision). Repeatable.
    #[arg(long = "at")]
    at: Vec<String>,
    /// Number of evenly spaced points when `--at` is not given.
    #[arg(long, default_value_t = 4)]
    points: u32,
    /// Days between points.
    #[arg(long, default_value_t = 90)]
    step_days: u32,
    /// Days after each point in which fixes are counted.
    #[arg(long, default_value_t = 180)]
    horizon_days: u32,
    /// Cut-offs for precision@k.
    #[arg(long, value_delimiter = ',', default_value = "10,20")]
    k: Vec<usize>,
    /// Copy the project's `.heal/semantic/` into each checkout so cached
    /// Jev verdicts take part in the drain order.
    #[arg(long)]
    with_verdicts: bool,
}

struct Point {
    label: String,
    oid: Oid,
    time: i64,
}

const DAY: i64 = 86_400;

fn main() -> Result<()> {
    let args = Args::parse();
    let project = args.project.canonicalize().context("--project")?;
    let repo = Repository::discover(&project).context("not a git repository")?;
    let head = repo.head()?.peel_to_commit()?;
    let horizon = i64::from(args.horizon_days) * DAY;
    let points = if args.at.is_empty() {
        spaced_points(&repo, head.time().seconds(), horizon, &args)?
    } else {
        args.at
            .iter()
            .map(|rev| {
                let c = repo.revparse_single(rev)?.peel_to_commit()?;
                Ok(Point {
                    label: rev.clone(),
                    oid: c.id(),
                    time: c.time().seconds(),
                })
            })
            .collect::<Result<Vec<_>>>()?
    };
    if points.is_empty() {
        bail!("history is shorter than one horizon; lower --horizon-days");
    }

    println!(
        "| point | files | fix touches | order | {} | fixes@20%LOC |",
        args.k
            .iter()
            .map(|k| format!("p@{k}"))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    println!("|---|---:|---:|---|{}---:|", "---:|".repeat(args.k.len()));
    let mut means: BTreeMap<&'static str, (Vec<f64>, f64, u32)> = BTreeMap::new();
    let mut counted = 0_usize;
    for point in &points {
        let result = evaluate(&project, &repo, point, horizon, &args)?;
        if result.fix_touches == 0 {
            println!(
                "| {} | {} | 0 | (no fixes in the horizon; excluded from the mean) | | |",
                point.label, result.files
            );
            continue;
        }
        counted += 1;
        for (order, (pk, effort)) in &result.scores {
            println!(
                "| {} | {} | {} | {order} | {} | {:.2} |",
                point.label,
                result.files,
                result.fix_touches,
                pk.iter()
                    .map(|p| format!("{p:.2}"))
                    .collect::<Vec<_>>()
                    .join(" | "),
                effort
            );
            let e = means
                .entry(order)
                .or_insert_with(|| (vec![0.0; args.k.len()], 0.0, 0));
            for (acc, p) in e.0.iter_mut().zip(pk) {
                *acc += p;
            }
            e.1 += effort;
            e.2 += 1;
        }
    }
    println!();
    println!("Mean over {counted} point(s) with at least one fix:");
    for (order, (pk, effort, n)) in means {
        let n = f64::from(n);
        println!(
            "  {order:<8} {}  fixes@20%LOC {:.2}",
            args.k
                .iter()
                .zip(&pk)
                .map(|(k, p)| format!("p@{k} {:.2}", p / n))
                .collect::<Vec<_>>()
                .join("  "),
            effort / n
        );
    }
    Ok(())
}

fn spaced_points(
    repo: &Repository,
    head_time: i64,
    horizon: i64,
    args: &Args,
) -> Result<Vec<Point>> {
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TIME)?;
    walk.push_head()?;
    let commits: Vec<(Oid, i64)> = walk
        .filter_map(Result::ok)
        .filter_map(|oid| {
            repo.find_commit(oid)
                .ok()
                .map(|c| (oid, c.time().seconds()))
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..args.points {
        let target = head_time - horizon - i64::from(i) * i64::from(args.step_days) * DAY;
        if let Some(&(oid, time)) = commits.iter().find(|(_, t)| *t <= target) {
            if out.iter().all(|p: &Point| p.oid != oid) {
                out.push(Point {
                    label: oid.to_string()[..8].to_owned(),
                    oid,
                    time,
                });
            }
        }
    }
    Ok(out)
}

struct Evaluation {
    files: usize,
    fix_touches: u32,
    scores: Vec<(&'static str, (Vec<f64>, f64))>,
}

fn evaluate(
    project: &Path,
    repo: &Repository,
    point: &Point,
    horizon: i64,
    args: &Args,
) -> Result<Evaluation> {
    let tmp = tempfile::tempdir()?;
    let worktree = tmp.path().join("wt");
    let _guard = Worktree::add(project, &worktree, &point.oid.to_string())?;
    let heal_src = project.join(".heal");
    let heal_dst = worktree.join(".heal");
    std::fs::create_dir_all(&heal_dst)?;
    std::fs::copy(heal_src.join("config.toml"), heal_dst.join("config.toml"))
        .context("the project needs .heal/config.toml (run `heal init`)")?;
    if args.with_verdicts && heal_src.join("semantic").is_dir() {
        copy_dir(&heal_src.join("semantic"), &heal_dst.join("semantic"))?;
    }
    let cfg = Config::load(&heal_dst.join("config.toml"))?;
    let (reports, findings) = heal_cli::observers::observe_self_calibrated(&worktree, &cfg);

    let universe: BTreeSet<PathBuf> = reports
        .complexity
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect();
    let loc: BTreeMap<PathBuf, u64> = universe
        .iter()
        .map(|p| {
            let n =
                std::fs::read_to_string(worktree.join(p)).map_or(0, |s| s.lines().count() as u64);
            (p.clone(), n)
        })
        .collect();
    let labels = fix_touches(repo, point.time, point.time + horizon, &universe)?;

    let mut orders: Vec<(&'static str, Vec<PathBuf>)> = Vec::new();
    orders.push(("heal", heal_order(&findings, &cfg, &universe)));
    let mut churn: Vec<(PathBuf, u32)> = reports
        .churn
        .as_ref()
        .map(|c| {
            c.files
                .iter()
                .map(|f| (f.path.clone(), f.commits))
                .collect()
        })
        .unwrap_or_default();
    churn.retain(|(p, _)| universe.contains(p));
    churn.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    orders.push((
        "churn",
        complete(churn.into_iter().map(|(p, _)| p).collect(), &universe),
    ));
    let mut by_loc: Vec<(&PathBuf, &u64)> = loc.iter().collect();
    by_loc.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    orders.push(("loc", by_loc.into_iter().map(|(p, _)| p.clone()).collect()));
    let mut random: Vec<PathBuf> = universe.iter().cloned().collect();
    random.sort_by_key(|p| fnv1a_64(p.to_string_lossy().as_bytes()));
    orders.push(("random", random));

    let total_fix: u32 = labels.values().sum();
    let total_loc: u64 = loc.values().sum();
    let scores = orders
        .into_iter()
        .map(|(name, order)| {
            let pk = args
                .k
                .iter()
                .map(|&k| {
                    let top = order
                        .iter()
                        .take(k)
                        .filter(|p| labels.get(*p).copied().unwrap_or(0) > 0)
                        .count();
                    #[allow(clippy::cast_precision_loss)]
                    let v = top as f64 / k.max(1) as f64;
                    v
                })
                .collect();
            (
                name,
                (
                    pk,
                    effort_share(&order, &labels, &loc, total_fix, total_loc),
                ),
            )
        })
        .collect();
    Ok(Evaluation {
        files: universe.len(),
        fix_touches: total_fix,
        scores,
    })
}

/// Files in drain order: the first appearance of each file among the
/// Tier-ranked (non-`Ok`) Findings, then every remaining file by path.
fn heal_order(findings: &[Finding], cfg: &Config, universe: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut ranked: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity != Severity::Ok)
        .collect();
    heal_cli::core::order::sort(&mut ranked, &cfg.policy.drain);
    let mut seen = BTreeSet::new();
    let head: Vec<PathBuf> = ranked
        .into_iter()
        .map(|f| f.location.file.clone())
        .filter(|p| universe.contains(p) && seen.insert(p.clone()))
        .collect();
    complete(head, universe)
}

fn complete(mut head: Vec<PathBuf>, universe: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let seen: BTreeSet<PathBuf> = head.iter().cloned().collect();
    head.extend(universe.iter().filter(|p| !seen.contains(*p)).cloned());
    head
}

/// Share of all fix touches captured while reading files in `order` until
/// 20% of total LOC has been read.
fn effort_share(
    order: &[PathBuf],
    labels: &BTreeMap<PathBuf, u32>,
    loc: &BTreeMap<PathBuf, u64>,
    total_fix: u32,
    total_loc: u64,
) -> f64 {
    if total_fix == 0 || total_loc == 0 {
        return 0.0;
    }
    let budget = total_loc / 5;
    let (mut read, mut found) = (0_u64, 0_u32);
    for p in order {
        if read >= budget {
            break;
        }
        read += loc.get(p).copied().unwrap_or(0);
        found += labels.get(p).copied().unwrap_or(0);
    }
    f64::from(found) / f64::from(total_fix)
}

fn is_fix(message: &str) -> bool {
    let subject = message.lines().next().unwrap_or("").to_ascii_lowercase();
    if subject.contains("(heal)") {
        return false;
    }
    let ty = subject.split([':', '(', '!']).next().unwrap_or("");
    if ty == "fix" {
        return true;
    }
    let words: Vec<&str> = subject
        .split(|c: char| !c.is_ascii_alphanumeric())
        .collect();
    words.iter().any(|w| {
        matches!(
            *w,
            "fix" | "fixes" | "fixed" | "bug" | "bugfix" | "hotfix" | "regression"
        )
    })
}

fn fix_touches(
    repo: &Repository,
    from: i64,
    to: i64,
    universe: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, u32>> {
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TIME)?;
    walk.push_head()?;
    let mut out = BTreeMap::new();
    for oid in walk.filter_map(Result::ok) {
        let commit = repo.find_commit(oid)?;
        let t = commit.time().seconds();
        if t <= from {
            break;
        }
        if t > to || commit.parent_count() > 1 || !is_fix(commit.message().unwrap_or("")) {
            continue;
        }
        let tree = commit.tree()?;
        let parent = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let diff = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), None)?;
        for delta in diff.deltas() {
            if let Some(p) = delta.old_file().path().or_else(|| delta.new_file().path()) {
                if universe.contains(p) {
                    *out.entry(p.to_path_buf()).or_insert(0) += 1;
                }
            }
        }
    }
    Ok(out)
}

struct Worktree {
    project: PathBuf,
    path: PathBuf,
}

impl Worktree {
    fn add(project: &Path, path: &Path, rev: &str) -> Result<Self> {
        let status = Command::new("git")
            .arg("-C")
            .arg(project)
            .args(["worktree", "add", "--detach", "--force"])
            .arg(path)
            .arg(rev)
            .output()?;
        if !status.status.success() {
            return Err(anyhow!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&status.stderr)
            ));
        }
        Ok(Self {
            project: project.to_path_buf(),
            path: path.to_path_buf(),
        })
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.project)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dst)?;
        } else {
            std::fs::copy(entry.path(), dst)?;
        }
    }
    Ok(())
}
