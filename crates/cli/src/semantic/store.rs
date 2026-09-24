//! Verdict cache: `.heal/semantic/verdicts/<task>.jsonl` for team
//! verdicts, `.heal/cache/semantic/verdicts/<task>.jsonl` (untracked) for
//! tasks that are not [`crate::semantic::task::Task::shared`].
//!
//! Only `heal semantic ask` writes here. Every other command reads the
//! cache and never calls the network, so `heal status` stays a pure
//! function of `(commit, config, calibration, observation inputs)` — the
//! verdict files are observation inputs and feed `config_hash`.
//!
//! One file per task, one JSON object per line, lines sorted by key. The
//! line-per-verdict layout keeps diffs small and lets teams mark the files
//! `merge=union` in `.gitattributes`; if a union merge leaves two verdicts
//! for one key, the lexicographically smallest line wins on read, so every
//! teammate resolves the conflict identically.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::hash::{fnv1a_64, fnv1a_64_chunked, fnv1a_hex};
use crate::semantic::api::Answer;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub key: String,
    pub model: String,
    pub answer: Answer,
}

/// Cache key of one question. Every input that could change the answer
/// is folded in: the task id, the hash of the task's criteria text, the
/// pinned model, the judged subject, and the shared state. Editing a
/// criterion, bumping the model, or changing the code all miss the cache.
#[must_use]
pub fn verdict_key(
    task: &str,
    criteria_hash: &str,
    model: &str,
    subject: &str,
    state: &str,
) -> String {
    fnv1a_hex(fnv1a_64_chunked(&[
        task.as_bytes(),
        criteria_hash.as_bytes(),
        model.as_bytes(),
        subject.as_bytes(),
        state.as_bytes(),
    ]))
}

/// Stable 16-hex digest of arbitrary content (source text, criteria).
#[must_use]
pub fn content_hash(bytes: &[u8]) -> String {
    fnv1a_hex(fnv1a_64(bytes))
}

#[derive(Debug)]
pub struct VerdictStore {
    dir: PathBuf,
    local: Option<LocalVerdicts>,
    tasks: BTreeMap<String, BTreeMap<String, Verdict>>,
    dirty: BTreeSet<String>,
}

/// Where the verdicts of non-shared tasks go instead of `dir`.
#[derive(Debug)]
struct LocalVerdicts {
    dir: PathBuf,
    /// Created (as `*`) when missing, so the directory stays untracked.
    gitignore: PathBuf,
    tasks: &'static BTreeSet<String>,
}

/// Ids of registered tasks whose verdicts are machine-local. Built once:
/// `config_hash` asks on every `heal status`, cache hits included.
#[must_use]
pub fn local_task_ids() -> &'static BTreeSet<String> {
    static IDS: std::sync::LazyLock<BTreeSet<String>> = std::sync::LazyLock::new(|| {
        crate::semantic::task::registry()
            .iter()
            .filter(|t| !t.shared())
            .map(|t| t.id().to_owned())
            .collect()
    });
    &IDS
}

impl VerdictStore {
    /// Every task in `dir`. Nothing is read until a task is first touched.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            local: None,
            tasks: BTreeMap::new(),
            dirty: BTreeSet::new(),
        }
    }

    /// The project's store: shared tasks in `.heal/semantic/verdicts`,
    /// the others in `.heal/cache/semantic/verdicts`.
    #[must_use]
    pub fn for_project(paths: &crate::core::HealPaths) -> Self {
        let mut store = Self::new(paths.semantic_verdicts());
        store.local = Some(LocalVerdicts {
            dir: paths.local_semantic_verdicts(),
            gitignore: paths.cache_gitignore(),
            tasks: local_task_ids(),
        });
        store
    }

    fn local_for(&self, task: &str) -> Option<&LocalVerdicts> {
        self.local.as_ref().filter(|l| l.tasks.contains(task))
    }

    fn file_for(&self, task: &str) -> PathBuf {
        let dir = self.local_for(task).map_or(&self.dir, |l| &l.dir);
        Self::path_for(dir, task)
    }

    #[must_use]
    pub fn path_for(dir: &Path, task: &str) -> PathBuf {
        dir.join(format!("{task}.jsonl"))
    }

    fn load(&mut self, task: &str) -> Result<&mut BTreeMap<String, Verdict>, String> {
        if !self.tasks.contains_key(task) {
            let map = read_file(&self.file_for(task))?;
            self.tasks.insert(task.to_owned(), map);
        }
        Ok(self.tasks.get_mut(task).expect("just inserted"))
    }

    pub fn get(&mut self, task: &str, key: &str) -> Result<Option<Verdict>, String> {
        Ok(self.load(task)?.get(key).cloned())
    }

    pub fn contains(&mut self, task: &str, key: &str) -> Result<bool, String> {
        Ok(self.load(task)?.contains_key(key))
    }

    pub fn insert(&mut self, task: &str, verdict: Verdict) -> Result<(), String> {
        let map = self.load(task)?;
        if map.get(&verdict.key) != Some(&verdict) {
            map.insert(verdict.key.clone(), verdict);
            self.dirty.insert(task.to_owned());
        }
        Ok(())
    }

    /// Drop every verdict of `task` whose key is not in `keep`. Returns the
    /// number removed. Keeps the tracked files from growing without bound
    /// as code changes and old subjects disappear.
    pub fn prune(&mut self, task: &str, keep: &BTreeSet<String>) -> Result<usize, String> {
        let map = self.load(task)?;
        let before = map.len();
        map.retain(|k, _| keep.contains(k));
        let removed = before - map.len();
        if removed > 0 {
            self.dirty.insert(task.to_owned());
        }
        Ok(removed)
    }

    /// Cached answers of `tasks`, for dependent tasks to read.
    pub fn snapshot(&mut self, tasks: &[&str]) -> Result<crate::semantic::task::Snapshot, String> {
        let mut out = crate::semantic::task::Snapshot::new();
        for task in tasks {
            let map = self.load(task)?;
            out.insert(
                (*task).to_owned(),
                map.iter()
                    .map(|(k, v)| (k.clone(), v.answer.clone()))
                    .collect(),
            );
        }
        Ok(out)
    }

    pub fn len(&mut self, task: &str) -> Result<usize, String> {
        Ok(self.load(task)?.len())
    }

    /// Write every modified task file atomically. An emptied task removes
    /// its file so a pruned-away task leaves no stub behind.
    pub fn save(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut written = Vec::new();
        for task in std::mem::take(&mut self.dirty) {
            let path = self.file_for(&task);
            if let Some(local) = self.local_for(&task) {
                if !local.gitignore.exists() {
                    crate::core::fs::atomic_write(&local.gitignore, b"*\n")
                        .map_err(|e| e.to_string())?;
                }
            }
            let map = &self.tasks[&task];
            if map.is_empty() {
                match std::fs::remove_file(&path) {
                    Ok(()) => written.push(path),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("{}: {e}", path.display())),
                }
                continue;
            }
            let mut body = String::new();
            for v in map.values() {
                body.push_str(&serde_json::to_string(v).expect("verdict serialization"));
                body.push('\n');
            }
            crate::core::fs::atomic_write(&path, body.as_bytes()).map_err(|e| e.to_string())?;
            written.push(path);
        }
        Ok(written)
    }
}

fn read_file(path: &Path) -> Result<BTreeMap<String, Verdict>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut best: BTreeMap<String, (String, Verdict)> = BTreeMap::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Verdict =
            serde_json::from_str(line).map_err(|e| format!("{}:{}: {e}", path.display(), i + 1))?;
        match best.get(&v.key) {
            Some((kept, _)) if kept.as_str() <= line => {}
            _ => {
                best.insert(v.key.clone(), (line.to_owned(), v));
            }
        }
    }
    Ok(best.into_iter().map(|(k, (_, v))| (k, v)).collect())
}

/// Every verdict file under `dir`, sorted by file name, for the
/// observation hash. Missing directory → empty.
#[must_use]
pub fn verdict_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl") && p.is_file())
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(key: &str, p: f64) -> Verdict {
        Verdict {
            key: key.to_owned(),
            model: "jev-1.13.0".to_owned(),
            answer: Answer::Noul { noul: p },
        }
    }

    #[test]
    fn key_changes_with_every_input() {
        let base = verdict_key("t", "c", "m", "s", "st");
        for other in [
            verdict_key("t2", "c", "m", "s", "st"),
            verdict_key("t", "c2", "m", "s", "st"),
            verdict_key("t", "c", "m2", "s", "st"),
            verdict_key("t", "c", "m", "s2", "st"),
            verdict_key("t", "c", "m", "s", "st2"),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn round_trip_is_sorted_and_byte_stable() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = VerdictStore::new(dir.path());
        s.insert("t", v("b", 0.2)).unwrap();
        s.insert("t", v("a", 0.9)).unwrap();
        s.save().unwrap();
        let body = std::fs::read_to_string(VerdictStore::path_for(dir.path(), "t")).unwrap();
        let keys: Vec<&str> = body.lines().map(|l| &l[8..9]).collect();
        assert_eq!(keys, ["a", "b"]);

        let mut again = VerdictStore::new(dir.path());
        again.insert("t", v("a", 0.9)).unwrap();
        assert!(
            again.save().unwrap().is_empty(),
            "unchanged insert must not rewrite"
        );
        assert_eq!(again.get("t", "b").unwrap(), Some(v("b", 0.2)));
    }

    #[test]
    fn non_shared_tasks_go_to_the_untracked_cache() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::core::HealPaths::new(dir.path());
        let mut s = VerdictStore::for_project(&paths);
        s.insert("verify_patch", v("a", 0.9)).unwrap();
        s.insert("focus", v("b", 0.2)).unwrap();
        s.insert("concept", v("c", 0.5)).unwrap();
        s.save().unwrap();

        let shared = paths.semantic_verdicts();
        let local = paths.local_semantic_verdicts();
        assert!(VerdictStore::path_for(&shared, "concept").exists());
        for task in ["verify_patch", "focus"] {
            assert!(VerdictStore::path_for(&local, task).exists(), "{task}");
            assert!(!VerdictStore::path_for(&shared, task).exists(), "{task}");
        }
        assert_eq!(
            std::fs::read_to_string(paths.cache_gitignore()).unwrap(),
            "*\n"
        );

        let mut again = VerdictStore::for_project(&paths);
        assert!(again.contains("verify_patch", "a").unwrap());
        assert!(again.contains("focus", "b").unwrap());
    }

    #[test]
    fn every_on_demand_task_is_local() {
        let local = local_task_ids();
        for t in crate::semantic::task::registry() {
            if t.on_demand() {
                assert!(local.contains(t.id()), "{}", t.id());
            }
            for dep in t.depends_on() {
                assert!(
                    !t.shared() || !local.contains(*dep),
                    "shared `{}` depends on local `{dep}`",
                    t.id()
                );
            }
        }
        assert!(local.contains("focus"));
    }

    #[test]
    fn duplicate_keys_resolve_to_the_smallest_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = VerdictStore::path_for(dir.path(), "t");
        let hi = serde_json::to_string(&v("a", 0.9)).unwrap();
        let lo = serde_json::to_string(&v("a", 0.1)).unwrap();
        std::fs::write(&path, format!("{hi}\n{lo}\n")).unwrap();
        let mut s = VerdictStore::new(dir.path());
        let got = s.get("t", "a").unwrap().unwrap();
        let expected = if lo < hi { 0.1 } else { 0.9 };
        assert_eq!(got.answer, Answer::Noul { noul: expected });
        std::fs::write(&path, format!("{lo}\n{hi}\n")).unwrap();
        let mut s = VerdictStore::new(dir.path());
        assert_eq!(
            s.get("t", "a").unwrap().unwrap().answer,
            Answer::Noul { noul: expected }
        );
    }

    #[test]
    fn prune_removes_unreferenced_and_deletes_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = VerdictStore::new(dir.path());
        s.insert("t", v("a", 0.5)).unwrap();
        s.insert("t", v("b", 0.5)).unwrap();
        s.save().unwrap();
        let keep: BTreeSet<String> = ["a".to_owned()].into();
        assert_eq!(s.prune("t", &keep).unwrap(), 1);
        s.save().unwrap();
        assert_eq!(s.len("t").unwrap(), 1);
        assert_eq!(s.prune("t", &BTreeSet::new()).unwrap(), 1);
        s.save().unwrap();
        assert!(!VerdictStore::path_for(dir.path(), "t").exists());
    }

    #[test]
    fn malformed_line_reports_its_position() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(VerdictStore::path_for(dir.path(), "t"), "{nope\n").unwrap();
        let err = VerdictStore::new(dir.path()).get("t", "a").unwrap_err();
        assert!(err.contains("t.jsonl:1"), "{err}");
    }

    #[test]
    fn verdict_files_are_sorted_jsonl_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("a.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("x.txt"), "").unwrap();
        let names: Vec<String> = verdict_files(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.jsonl", "b.jsonl"]);
        assert!(verdict_files(&dir.path().join("missing")).is_empty());
    }
}
