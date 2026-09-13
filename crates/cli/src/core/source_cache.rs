use std::collections::BTreeMap;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const SOURCE_CACHE_VERSION: u32 = 1;

fn regular_file_or_missing(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_file(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

fn local_cache_path(path: &Path) -> bool {
    // The two owned directories are .heal/cache and .heal. Do not follow
    // either into another location when creating disposable analysis state.
    path.parent().is_some_and(|dir| {
        dir.ancestors()
            .take(2)
            .all(|ancestor| match std::fs::symlink_metadata(ancestor) {
                Ok(metadata) => metadata.is_dir(),
                Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            })
    }) && regular_file_or_missing(path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceCache<T> {
    version: u32,
    pub entries: BTreeMap<String, SourceCacheEntry<T>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceCacheEntry<T> {
    pub size: u64,
    pub content_hash: u64,
    pub language: String,
    pub analyzer_version: u32,
    pub capabilities: u8,
    pub data: T,
}

impl<T> Default for SourceCache<T> {
    fn default() -> Self {
        Self {
            version: SOURCE_CACHE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

impl<T: DeserializeOwned> SourceCache<T> {
    pub fn load(path: &Path) -> Self {
        if !local_cache_path(path) {
            return Self::default();
        }
        let Ok(body) = std::fs::read(path) else {
            return Self::default();
        };
        serde_json::from_slice(&body)
            .ok()
            .filter(|cache: &Self| cache.version == SOURCE_CACHE_VERSION)
            .unwrap_or_default()
    }
}

impl<T: Serialize> SourceCache<T> {
    pub fn save(&self, path: &Path) {
        if !local_cache_path(path) {
            return;
        }
        let Some(dir) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let ignore_path = dir.join(".gitignore");
        if !regular_file_or_missing(&ignore_path) {
            return;
        }
        if !ignore_path.exists() {
            let _ = crate::core::fs::atomic_write(&ignore_path, b"*\n");
        }
        let Ok(ignore_body) = std::fs::read_to_string(&ignore_path) else {
            return;
        };
        let mut ignore = ignore::gitignore::GitignoreBuilder::new(dir);
        for line in ignore_body.lines() {
            if ignore.add_line(None, line).is_err() {
                return;
            }
        }
        let Ok(ignore) = ignore.build() else {
            return;
        };
        if !ignore.matched_path_or_any_parents(path, false).is_ignore() {
            return;
        }
        let Ok(body) = serde_json::to_vec(self) else {
            return;
        };
        let _ = crate::core::fs::atomic_write(path, &body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_or_old_cache_falls_back_to_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("cache/source.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json").unwrap();
        assert!(SourceCache::<String>::load(&path).entries.is_empty());
        std::fs::write(&path, br#"{"version":999,"entries":{}}"#).unwrap();
        assert!(SourceCache::<String>::load(&path).entries.is_empty());
    }

    #[test]
    fn save_is_ignored_and_round_trips() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".heal/cache/source.json");
        let mut cache = SourceCache::default();
        cache.entries.insert(
            "src/lib.rs".into(),
            SourceCacheEntry {
                size: 3,
                content_hash: 7,
                language: "rust".into(),
                analyzer_version: 1,
                capabilities: 1,
                data: "payload".to_owned(),
            },
        );
        cache.save(&path);
        assert_eq!(SourceCache::<String>::load(&path).entries.len(), 1);
        assert_eq!(
            std::fs::read_to_string(path.parent().unwrap().join(".gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn save_does_not_create_an_unignored_cache_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let cache_dir = dir.path().join(".heal/cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join(".gitignore"), "other.json\n").unwrap();
        let path = cache_dir.join("source-v1.json");
        SourceCache::<String>::default().save(&path);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_directories_do_not_read_or_write_outside_state() {
        use std::os::unix::fs::symlink;

        for link_heal in [false, true] {
            let dir = tempfile::TempDir::new().unwrap();
            let project = dir.path().join("project");
            let elsewhere = dir.path().join("elsewhere");
            std::fs::create_dir_all(&project).unwrap();
            let external_cache = if link_heal {
                std::fs::create_dir_all(elsewhere.join("cache")).unwrap();
                symlink(&elsewhere, project.join(".heal")).unwrap();
                elsewhere.join("cache")
            } else {
                std::fs::create_dir_all(project.join(".heal")).unwrap();
                std::fs::create_dir_all(&elsewhere).unwrap();
                symlink(&elsewhere, project.join(".heal/cache")).unwrap();
                elsewhere.clone()
            };
            let external_path = external_cache.join("source-v1.json");
            let body = br#"{"version":1,"entries":{"foreign":{"size":1,"content_hash":1,"language":"rust","analyzer_version":2,"capabilities":1,"data":"foreign"}}}"#;
            std::fs::write(&external_path, body).unwrap();
            let path = project.join(".heal/cache/source-v1.json");

            assert!(SourceCache::<String>::load(&path).entries.is_empty());
            SourceCache::<String>::default().save(&path);

            assert_eq!(std::fs::read(&external_path).unwrap(), body);
            assert!(!external_cache.join(".gitignore").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_files_and_ignore_files_are_preserved() {
        use std::os::unix::fs::symlink;

        for link_ignore in [false, true] {
            let dir = tempfile::TempDir::new().unwrap();
            let cache_dir = dir.path().join(".heal/cache");
            std::fs::create_dir_all(&cache_dir).unwrap();
            let external = dir.path().join("external");
            std::fs::write(&external, b"*\n").unwrap();
            let path = cache_dir.join("source-v1.json");
            let linked_path = if link_ignore {
                cache_dir.join(".gitignore")
            } else {
                path.clone()
            };
            symlink(&external, &linked_path).unwrap();

            SourceCache::<String>::default().save(&path);

            assert!(std::fs::symlink_metadata(&linked_path)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(std::fs::read(&external).unwrap(), b"*\n");
            if link_ignore {
                assert!(!path.exists());
            }
        }
    }
}
