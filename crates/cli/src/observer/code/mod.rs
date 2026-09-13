//! Code-feature observer family. Reads project source + git history and
//! emits the v0.x code-substrate Findings. Per-metric on/off lives under
//! `[metrics.<m>]` in `.heal/config.toml`.

pub mod change_coupling;
pub mod churn;
pub mod complexity;
pub mod duplication;
pub mod hotspot;
pub mod lcom;
pub mod loc;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Condvar, Mutex};

use serde::{Deserialize, Serialize};

use crate::observer::shared::lang::Language;
use crate::observer::shared::walk::{walk_supported_files_under, ExcludeMatcher};

/// Walk the source tree once, parse each supported file once, and feed
/// every requested accumulator. Complexity, Duplication, and LCOM all
/// walk the same file set (same `cfg.exclude_lines()`, same workspace
/// scope in the orchestrator), so scanning them together removes an
/// up-to-3× read + tree-sitter-parse cost per run. Each observer's
/// `scan` delegates here with only its own accumulator, so standalone
/// behavior is unchanged.
pub(crate) fn scan_source_tree(
    root: &Path,
    excluded: &[String],
    workspace: Option<&Path>,
    complexity: Option<&mut complexity::ComplexityAccumulator>,
    duplication: Option<&mut duplication::DuplicationAccumulator>,
    lcom: Option<&mut lcom::LcomAccumulator>,
) {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(8);
    scan_source_tree_with_workers(
        root,
        excluded,
        workspace,
        complexity,
        duplication,
        lcom,
        workers,
    );
}

struct AnalyzedFile {
    rel: PathBuf,
    lang: Language,
    cache_key: Option<String>,
    entry: crate::core::source_cache::SourceCacheEntry<CachedAnalysis>,
    cache_hit: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CachedAnalysis {
    complexity: Option<Vec<complexity::FunctionMetric>>,
    duplication: Option<(Vec<u64>, Vec<u32>)>,
    lcom: Option<Vec<lcom::ClassLcom>>,
    #[serde(default)]
    checksum: u64,
}

impl CachedAnalysis {
    fn sealed(mut self) -> Self {
        self.checksum = self.computed_checksum();
        self
    }

    fn computed_checksum(&self) -> u64 {
        serde_json::to_vec(&(&self.complexity, &self.duplication, &self.lcom))
            .map_or(0, |bytes| crate::core::hash::fnv1a_64(&bytes))
    }

    fn matches_entry(&self, capabilities: u8, rel: &Path, lang: Language) -> bool {
        self.checksum != 0
            && self.checksum == self.computed_checksum()
            && capabilities & !(CACHE_COMPLEXITY | CACHE_DUPLICATION | CACHE_LCOM) == 0
            && self.complexity.is_some() == (capabilities & CACHE_COMPLEXITY != 0)
            && self.duplication.is_some() == (capabilities & CACHE_DUPLICATION != 0)
            && self.lcom.is_some() == (capabilities & CACHE_LCOM != 0)
            && self.complexity.as_ref().is_none_or(|metrics| {
                metrics.iter().all(|metric| {
                    metric.ccn > 0 && metric.start_line > 0 && metric.end_line >= metric.start_line
                })
            })
            && self.duplication.as_ref().is_none_or(|(hashes, lines)| {
                hashes.len() == lines.len() && lines.iter().all(|line| *line > 0)
            })
            && self.lcom.as_ref().is_none_or(|classes| {
                classes.iter().all(|class| {
                    class.file == rel
                        && class.language == lang.name()
                        && class.start_line > 0
                        && class.end_line >= class.start_line
                        && class.method_count > 0
                        && class.cluster_count > 0
                        && usize::try_from(class.cluster_count).ok() == Some(class.clusters.len())
                        && class
                            .clusters
                            .iter()
                            .all(|cluster| !cluster.methods.is_empty())
                        && class.cluster_count <= class.method_count
                })
            })
    }
}

const CACHE_ANALYZER_VERSION: u32 = 2;
const CACHE_COMPLEXITY: u8 = 1;
const CACHE_DUPLICATION: u8 = 2;
const CACHE_LCOM: u8 = 4;

#[derive(Debug, Default, PartialEq, Eq)]
struct SourceScanStats {
    parsed: usize,
    cache_hits: usize,
    max_pending: usize,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn scan_source_tree_with_workers(
    root: &Path,
    excluded: &[String],
    workspace: Option<&Path>,
    mut complexity: Option<&mut complexity::ComplexityAccumulator>,
    mut duplication: Option<&mut duplication::DuplicationAccumulator>,
    mut lcom: Option<&mut lcom::LcomAccumulator>,
    workers: usize,
) -> SourceScanStats {
    if complexity.is_none() && duplication.is_none() && lcom.is_none() {
        return SourceScanStats::default();
    }
    let matcher =
        ExcludeMatcher::compile(root, excluded).expect("exclude patterns validated at config load");
    let files = walk_supported_files_under(root, &matcher, workspace);
    if files.is_empty() {
        return SourceScanStats::default();
    }
    let worker_count = workers.max(1).min(files.len());
    let next = AtomicUsize::new(0);
    let result_window = worker_count * 2;
    let (sender, receiver) = mpsc::sync_channel(result_window);
    let release_gate = (Mutex::new(result_window), Condvar::new());
    let need_complexity = complexity.is_some();
    let need_duplication = duplication.is_some();
    let need_lcom = lcom.is_some();
    let required_capabilities = (u8::from(need_complexity) * CACHE_COMPLEXITY)
        | (u8::from(need_duplication) * CACHE_DUPLICATION)
        | (u8::from(need_lcom) * CACHE_LCOM);
    let cache_path = crate::core::HealPaths::new(root).source_cache();
    let cache_enabled = crate::core::HealPaths::new(root).config().is_file();
    let previous = if cache_enabled {
        crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&cache_path)
    } else {
        crate::core::source_cache::SourceCache::default()
    };
    let mut next_cache = crate::core::source_cache::SourceCache::default();
    let mut stats = SourceScanStats::default();
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let files = &files;
            let next = &next;
            let previous = &previous;
            let release_gate = &release_gate;
            scope.spawn(move || {
                let mut parsers = complexity::ParserPool::default();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(index) else {
                        break;
                    };
                    let (limit, advanced) = release_gate;
                    let limit = limit.lock().expect("result-window lock poisoned");
                    let limit = advanced
                        .wait_while(limit, |limit| index >= *limit)
                        .expect("result-window lock poisoned");
                    drop(limit);
                    let analyzed = (|| {
                        let lang = Language::from_path(path)
                            .expect("walker filters by Language::from_path");
                        let source = std::fs::read_to_string(path).ok()?;
                        let rel = path
                            .strip_prefix(root)
                            .map_or_else(|_| path.clone(), Path::to_path_buf);
                        let cache_key = rel.to_str().map(str::to_owned);
                        let content_hash = crate::core::hash::fnv1a_64(source.as_bytes());
                        let size = u64::try_from(source.len()).unwrap_or(u64::MAX);
                        if let Some(entry) = cache_key
                            .as_ref()
                            .and_then(|key| previous.entries.get(key))
                            .filter(|entry| {
                                entry.size == size
                                    && entry.content_hash == content_hash
                                    && entry.language == lang.name()
                                    && entry.analyzer_version == CACHE_ANALYZER_VERSION
                                    && entry.capabilities & required_capabilities
                                        == required_capabilities
                                    && entry.data.matches_entry(entry.capabilities, &rel, lang)
                            })
                        {
                            return Some(AnalyzedFile {
                                rel,
                                lang,
                                cache_key,
                                entry: entry.clone(),
                                cache_hit: true,
                            });
                        }
                        let parsed = parsers.parse(source, lang).ok()?;
                        let data = CachedAnalysis {
                            complexity: need_complexity.then(|| complexity::analyze(&parsed)),
                            duplication: need_duplication
                                .then(|| duplication::collect_tokens(&parsed)),
                            lcom: need_lcom.then(|| lcom::classes_in(&parsed, &rel)),
                            checksum: 0,
                        }
                        .sealed();
                        Some(AnalyzedFile {
                            rel,
                            lang,
                            cache_key,
                            entry: crate::core::source_cache::SourceCacheEntry {
                                size,
                                content_hash,
                                language: lang.name().to_owned(),
                                analyzer_version: CACHE_ANALYZER_VERSION,
                                capabilities: required_capabilities,
                                data,
                            },
                            cache_hit: false,
                        })
                    })();
                    if sender.send((index, analyzed)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut pending = BTreeMap::new();
        let mut expected = 0;
        let mut all_hit = true;
        while let Ok((index, analyzed)) = receiver.recv() {
            pending.insert(index, analyzed);
            stats.max_pending = stats.max_pending.max(pending.len());
            let mut consumed = false;
            while let Some(analyzed) = pending.remove(&expected) {
                if let Some(analyzed) = analyzed {
                    all_hit &= analyzed.cache_hit;
                    if analyzed.cache_hit {
                        stats.cache_hits += 1;
                    } else {
                        stats.parsed += 1;
                    }
                    if let (Some(acc), Some(metrics)) = (
                        complexity.as_deref_mut(),
                        analyzed.entry.data.complexity.clone(),
                    ) {
                        acc.add_metrics(&analyzed.rel, analyzed.lang, metrics);
                    }
                    if let (Some(acc), Some((hashes, lines))) = (
                        duplication.as_deref_mut(),
                        analyzed.entry.data.duplication.clone(),
                    ) {
                        acc.add_tokens(&analyzed.rel, hashes, lines);
                    }
                    if let (Some(acc), Some(classes)) =
                        (lcom.as_deref_mut(), analyzed.entry.data.lcom.clone())
                    {
                        acc.add_classes(classes);
                    }
                    if let Some(cache_key) = analyzed.cache_key {
                        next_cache.entries.insert(cache_key, analyzed.entry);
                    }
                }
                expected += 1;
                consumed = true;
            }
            if consumed {
                let (limit, advanced) = &release_gate;
                *limit.lock().expect("result-window lock poisoned") =
                    expected.saturating_add(result_window);
                advanced.notify_all();
            }
        }
        debug_assert!(stats.max_pending <= result_window);
        if cache_enabled
            && stats.parsed + stats.cache_hits == files.len()
            && (!all_hit || previous.entries.len() != next_cache.entries.len())
        {
            next_cache.save(&cache_path);
        }
    });
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::observer::code::complexity::ComplexityObserver;
    use crate::observer::code::duplication::DuplicationObserver;
    use crate::observer::code::lcom::LcomObserver;

    fn run_cached_scan(root: &Path, all: bool) -> (complexity::ComplexityReport, SourceScanStats) {
        let cfg = Config::default();
        let complexity_observer = ComplexityObserver::from_config(&cfg);
        let duplication_observer = DuplicationObserver::from_config(&cfg);
        let lcom_observer = LcomObserver::from_config(&cfg);
        let mut complexity = complexity_observer.accumulator().unwrap();
        let mut duplication = all.then(|| duplication_observer.accumulator().unwrap());
        let mut lcom = all.then(|| lcom_observer.accumulator().unwrap());
        let stats = scan_source_tree_with_workers(
            root,
            &complexity_observer.excluded,
            None,
            Some(&mut complexity),
            duplication.as_mut(),
            lcom.as_mut(),
            2,
        );
        (complexity.finish(), stats)
    }

    #[test]
    fn parallel_scan_matches_single_worker_in_input_order() {
        let dir = tempfile::TempDir::new().unwrap();
        for (name, body) in [
            ("a.ts", "class A { x = 1; f() { if (this.x) return 1; } }\n"),
            ("b.tsx", "const B = () => <div>{true ? 'x' : 'y'}</div>;\n"),
            ("c.js", "function c(x) { while (x--) { if (x) break; } }\n"),
            ("d.py", "class D:\n def f(self, x):\n  if x:\n   return x\n"),
            ("e.go", "package p\nfunc E(x int) int { if x > 0 { return x }; return 0 }\n"),
            ("f.scala", "object F { def f(x: Int) = if (x > 0) x else 0 }\n"),
            ("g.rs", "struct G { x: i32 } impl G { fn f(&self) -> i32 { if self.x > 0 { self.x } else { 0 } } }\n"),
            ("h.rs", "fn partial( { if true {\n"),
        ] {
            std::fs::write(dir.path().join(name), body).unwrap();
        }

        let run = |workers| {
            let cfg = Config::default();
            let complexity_observer = ComplexityObserver::from_config(&cfg);
            let duplication_observer = DuplicationObserver::from_config(&cfg);
            let lcom_observer = LcomObserver::from_config(&cfg);
            let mut complexity = complexity_observer.accumulator().unwrap();
            let mut duplication = duplication_observer.accumulator().unwrap();
            let mut lcom = lcom_observer.accumulator().unwrap();
            scan_source_tree_with_workers(
                dir.path(),
                &complexity_observer.excluded,
                None,
                Some(&mut complexity),
                Some(&mut duplication),
                Some(&mut lcom),
                workers,
            );
            (
                complexity.finish(),
                duplication_observer.finish(duplication),
                lcom_observer.finish(lcom),
            )
        };

        assert_eq!(run(1), run(4));
    }

    #[test]
    fn cache_hits_expand_capabilities_and_validate_content_not_mtime() {
        use crate::test_support::{git, init_repo};

        let dir = tempfile::TempDir::new().unwrap();
        init_repo(dir.path());
        let paths = crate::core::HealPaths::new(dir.path());
        paths.ensure().unwrap();
        Config::default().save(&paths.config()).unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        let source_path = dir.path().join(source_name);
        std::fs::write(&source_path, source).unwrap();
        git(dir.path(), &["add", ".heal/config.toml", source_name]);
        git(
            dir.path(),
            &[
                "-c",
                "user.name=tester",
                "-c",
                "user.email=tester@example.com",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
        );

        let (first, cold_stats) = run_cached_scan(dir.path(), false);
        assert_eq!(
            cold_stats,
            SourceScanStats {
                parsed: 1,
                cache_hits: 0,
                max_pending: 1,
            }
        );
        let cache_path = paths.source_cache();
        let first_cache =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&cache_path);
        assert_eq!(
            first_cache.entries[source_name].capabilities,
            CACHE_COMPLEXITY
        );
        assert_eq!(
            crate::observer::shared::git::worktree_clean(dir.path()),
            Some(true)
        );
        let cache_modified = std::fs::metadata(&cache_path).unwrap().modified().unwrap();
        let (hit, hit_stats) = run_cached_scan(dir.path(), false);
        assert_eq!(hit, first);
        assert_eq!(
            hit_stats,
            SourceScanStats {
                parsed: 0,
                cache_hits: 1,
                max_pending: 1,
            }
        );
        assert_eq!(
            std::fs::metadata(&cache_path).unwrap().modified().unwrap(),
            cache_modified,
            "all-hit scans must not rewrite the cache",
        );

        let _ = run_cached_scan(dir.path(), true);
        let full_cache =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&cache_path);
        assert_eq!(
            full_cache.entries[source_name].capabilities,
            CACHE_COMPLEXITY | CACHE_DUPLICATION | CACHE_LCOM,
        );

        let original_mtime = std::fs::metadata(&source_path).unwrap().modified().unwrap();
        let changed = source.replacen('1', "2", 1);
        assert_eq!(changed.len(), source.len());
        std::fs::write(&source_path, changed).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&source_path)
            .unwrap()
            .set_modified(original_mtime)
            .unwrap();
        let _ = run_cached_scan(dir.path(), true);
        let changed_cache =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&cache_path);
        assert_ne!(
            changed_cache.entries[source_name].content_hash,
            full_cache.entries[source_name].content_hash,
        );
    }

    #[allow(clippy::too_many_lines)]
    #[test]
    fn corrupt_unwritable_and_concurrent_cache_fall_back_to_analysis() {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = crate::core::HealPaths::new(dir.path());
        paths.ensure().unwrap();
        Config::default().save(&paths.config()).unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        std::fs::write(dir.path().join(source_name), source).unwrap();
        let expected = run_cached_scan(dir.path(), true).0;

        let mut invalid_payload =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&paths.source_cache());
        invalid_payload
            .entries
            .get_mut(source_name)
            .unwrap()
            .data
            .complexity = None;
        std::fs::write(
            paths.source_cache(),
            serde_json::to_vec(&invalid_payload).unwrap(),
        )
        .unwrap();
        let (actual, stats) = run_cached_scan(dir.path(), true);
        assert_eq!(actual, expected);
        assert_eq!(stats.parsed, 1);
        assert_eq!(stats.cache_hits, 0);

        let mut invalid_payload =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&paths.source_cache());
        invalid_payload
            .entries
            .get_mut(source_name)
            .unwrap()
            .data
            .duplication = Some((vec![1], Vec::new()));
        std::fs::write(
            paths.source_cache(),
            serde_json::to_vec(&invalid_payload).unwrap(),
        )
        .unwrap();
        let (actual, stats) = run_cached_scan(dir.path(), true);
        assert_eq!(actual, expected);
        assert_eq!(stats.parsed, 1);
        assert_eq!(stats.cache_hits, 0);

        let mut invalid_payload =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&paths.source_cache());
        let entry = invalid_payload.entries.get_mut(source_name).unwrap();
        entry.data.lcom = Some(vec![lcom::ClassLcom {
            file: PathBuf::from("different-file.rs"),
            language: entry.language.clone(),
            class_name: "Injected".into(),
            start_line: 1,
            end_line: 1,
            method_count: 1,
            cluster_count: 1,
            clusters: vec![lcom::MethodCluster {
                methods: vec!["method".into()],
            }],
        }]);
        std::fs::write(
            paths.source_cache(),
            serde_json::to_vec(&invalid_payload).unwrap(),
        )
        .unwrap();
        let (actual, stats) = run_cached_scan(dir.path(), true);
        assert_eq!(actual, expected);
        assert_eq!(stats.parsed, 1);
        assert_eq!(stats.cache_hits, 0);

        let mut invalid_payload =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&paths.source_cache());
        let entry = invalid_payload.entries.get_mut(source_name).unwrap();
        entry.data.lcom = Some(vec![lcom::ClassLcom {
            file: PathBuf::from(source_name),
            language: entry.language.clone(),
            class_name: "Injected".into(),
            start_line: 1,
            end_line: 1,
            method_count: 0,
            cluster_count: 1,
            clusters: vec![lcom::MethodCluster {
                methods: vec!["method".into()],
            }],
        }]);
        std::fs::write(
            paths.source_cache(),
            serde_json::to_vec(&invalid_payload).unwrap(),
        )
        .unwrap();
        let (actual, stats) = run_cached_scan(dir.path(), true);
        assert_eq!(actual, expected);
        assert_eq!(stats.parsed, 1);
        assert_eq!(stats.cache_hits, 0);

        let mut invalid_payload =
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&paths.source_cache());
        invalid_payload
            .entries
            .get_mut(source_name)
            .unwrap()
            .data
            .complexity
            .as_mut()
            .unwrap()[0]
            .ccn = 0;
        std::fs::write(
            paths.source_cache(),
            serde_json::to_vec(&invalid_payload).unwrap(),
        )
        .unwrap();
        let (actual, stats) = run_cached_scan(dir.path(), true);
        assert_eq!(actual, expected);
        assert_eq!(stats.parsed, 1);
        assert_eq!(stats.cache_hits, 0);

        std::fs::write(paths.source_cache(), b"corrupt").unwrap();
        assert_eq!(run_cached_scan(dir.path(), true).0, expected);

        std::fs::remove_file(paths.source_cache()).unwrap();
        let cache_dir = paths.source_cache().parent().unwrap().to_path_buf();
        std::fs::remove_file(cache_dir.join(".gitignore")).unwrap();
        std::fs::remove_dir(&cache_dir).unwrap();
        std::fs::write(&cache_dir, b"not a directory").unwrap();
        assert_eq!(run_cached_scan(dir.path(), true).0, expected);
        std::fs::remove_file(&cache_dir).unwrap();

        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| assert_eq!(run_cached_scan(dir.path(), true).0, expected));
            }
        });
        assert_eq!(run_cached_scan(dir.path(), true).0, expected);
        assert_eq!(
            crate::core::source_cache::SourceCache::<CachedAnalysis>::load(&paths.source_cache())
                .entries
                .len(),
            1,
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn non_utf8_source_path_is_analyzed_without_caching() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let dir = tempfile::TempDir::new().unwrap();
        let paths = crate::core::HealPaths::new(dir.path());
        paths.ensure().unwrap();
        Config::default().save(&paths.config()).unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        let mut name = vec![b'f', 0xff, b'.'];
        name.extend_from_slice(Path::new(source_name).extension().unwrap().as_bytes());
        let name = std::ffi::OsString::from_vec(name);
        std::fs::write(dir.path().join(name), source).unwrap();

        let (report, stats) = run_cached_scan(dir.path(), false);
        assert_eq!(stats.parsed, 1);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(report.totals.files, 1);
    }

    #[cfg(feature = "lang-javascript")]
    #[test]
    fn cache_accepts_lcom_with_duplicate_method_names() {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = crate::core::HealPaths::new(dir.path());
        paths.ensure().unwrap();
        Config::default().save(&paths.config()).unwrap();
        std::fs::write(
            dir.path().join("duplicate.js"),
            "class Example { same() { this.x = 1; } same() { this.x = 2; } }\n",
        )
        .unwrap();

        let (_, cold) = run_cached_scan(dir.path(), true);
        let (_, hit) = run_cached_scan(dir.path(), true);
        assert_eq!(cold.parsed, 1);
        assert_eq!(hit.cache_hits, 1);
        assert_eq!(hit.parsed, 0);
    }
}
